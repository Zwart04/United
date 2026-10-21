using System.IO.Pipes;
using System.Text;

namespace United.Platform.Windows.Activation;

/// <summary>
/// Single-instance guard + activation forwarding over a named pipe.
/// First process becomes the primary and listens; later launches forward their
/// command line (protocol URI etc.) and exit immediately.
/// </summary>
public sealed class SingleInstance : IDisposable
{
    private const string MutexName = @"Local\UnitedMessenger.SingleInstance";
    private const string PipeName = "UnitedMessenger.Activation";

    private readonly Mutex _mutex;
    private readonly bool _isPrimary;
    private CancellationTokenSource? _cts;
    private Task? _serverTask;

    public event Action<string>? MessageReceived;

    public SingleInstance()
    {
        _mutex = new Mutex(initiallyOwned: true, MutexName, out var createdNew);
        _isPrimary = createdNew;
    }

    public bool IsPrimary => _isPrimary;

    public void StartServer()
    {
        if (!_isPrimary) return;
        _cts = new CancellationTokenSource();
        _serverTask = Task.Run(() => ServerLoopAsync(_cts.Token));
    }

    private async Task ServerLoopAsync(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            try
            {
                await using var server = new NamedPipeServerStream(
                    PipeName, PipeDirection.In, maxNumberOfServerInstances: 1,
                    PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
                await server.WaitForConnectionAsync(token);

                using var reader = new StreamReader(server, Encoding.UTF8);
                var line = await reader.ReadLineAsync(token);
                if (!string.IsNullOrWhiteSpace(line))
                    MessageReceived?.Invoke(line);
            }
            catch (OperationCanceledException)
            {
                break;
            }
            catch (IOException)
            {
                // A connecting client vanished; keep serving.
            }
        }
    }

    /// <summary>Forwards a message to the running primary instance. Safe to call from a secondary process.</summary>
    public static bool ForwardToPrimary(string message, int timeoutMs = 2000)
    {
        var deadline = DateTime.UtcNow.AddMilliseconds(timeoutMs);
        while (DateTime.UtcNow < deadline)
        {
            try
            {
                using var client = new NamedPipeClientStream(
                    ".", PipeName, PipeDirection.Out, PipeOptions.Asynchronous);
                client.Connect(Math.Max(150, timeoutMs / 4));
                using var writer = new StreamWriter(client, Encoding.UTF8);
                writer.WriteLine(message);
                writer.Flush();
                return true;
            }
            catch (TimeoutException)
            {
                return false;
            }
            catch (IOException)
            {
                Thread.Sleep(100);
            }
            catch (System.Net.Sockets.SocketException)
            {
                Thread.Sleep(100);
            }
        }
        return false;
    }

    public void Dispose()
    {
        try { _cts?.Cancel(); } catch { /* shutting down */ }
        try { _serverTask?.Wait(300); } catch { /* shutting down */ }
        _cts?.Dispose();
        if (_isPrimary)
        {
            try { _mutex.ReleaseMutex(); } catch { /* abandoned */ }
        }
        _mutex.Dispose();
    }
}
