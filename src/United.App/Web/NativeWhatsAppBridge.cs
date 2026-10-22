using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using United.App.Diagnostics;
using United.Core.Settings;

namespace United.App.Web;

/// <summary>One message preview written by the native WhatsApp bridge.</summary>
public sealed class NativeWhatsAppMessage
{
    public string Type { get; set; } = "";
    public string Title { get; set; } = "";
    public string Body { get; set; } = "";
    public DateTimeOffset At { get; set; }
    public string? Chat { get; set; }
    public string? Sender { get; set; }
}

/// <summary>Pairing/connection state written by the native bridge.</summary>
public sealed class NativeWhatsAppStatus
{
    public string State { get; set; } = "stopped";
    public bool Paired { get; set; }
    public DateTimeOffset UpdatedAt { get; set; }
    public string? Error { get; set; }
    public string? Me { get; set; }
}

/// <summary>
/// Owns the optional Go helper that links a WhatsApp companion device and emits
/// JSONL previews. The helper is intentionally a separate process; if it is
/// missing or fails, the normal WhatsApp WebView remains usable.
/// </summary>
public sealed class NativeWhatsAppBridge : IDisposable
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    private readonly AppSettings _settings;
    private readonly object _gate = new();
    private readonly StringBuilder _pending = new();
    private Process? _process;
    private CancellationTokenSource? _cts;
    private long _eventsOffset;
    private NativeWhatsAppStatus _status = new();
    private bool _disposed;

    public NativeWhatsAppBridge(AppSettings settings)
    {
        _settings = settings;
        DataDirectory = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "United", "WhatsAppNative");
        EventsPath = Path.Combine(DataDirectory, "events.jsonl");
        StatusPath = Path.Combine(DataDirectory, "status.json");
        QrPath = Path.Combine(DataDirectory, "qr.png");
        PidPath = Path.Combine(DataDirectory, "bridge.pid");
        LogPath = Path.Combine(DataDirectory, "bridge.log");
        ExecutablePath = Path.Combine(AppContext.BaseDirectory, "United.WhatsAppBridge.exe");
    }

    public string DataDirectory { get; }
    public string EventsPath { get; }
    public string StatusPath { get; }
    public string QrPath { get; }
    public string PidPath { get; }
    public string LogPath { get; }
    public string ExecutablePath { get; }

    public bool IsEnabled => _settings.NativeWhatsAppEnabled;
    public bool IsAvailable => File.Exists(ExecutablePath);
    public bool IsRunning
    {
        get
        {
            lock (_gate) return _process is { HasExited: false };
        }
    }

    public NativeWhatsAppStatus Status
    {
        get
        {
            lock (_gate)
                return new NativeWhatsAppStatus
                {
                    State = _status.State,
                    Paired = _status.Paired,
                    UpdatedAt = _status.UpdatedAt,
                    Error = _status.Error,
                    Me = _status.Me,
                };
        }
    }

    /// <summary>
    /// True only when the bridge has a linked device and the session database is
    /// actually present. A leftover status file must not hide the WebView.
    /// </summary>
    public bool IsPaired => Status.Paired && File.Exists(Path.Combine(DataDirectory, "session.db"));
    public bool IsConnected => string.Equals(Status.State, "connected", StringComparison.OrdinalIgnoreCase);

    public event Action<NativeWhatsAppMessage>? MessageObserved;
    public event Action? StatusChanged;

    public void Start()
    {
        if (_disposed || !IsEnabled || !IsAvailable) return;

        lock (_gate)
        {
            if (_process is { HasExited: false }) return;
            Directory.CreateDirectory(DataDirectory);
            KillStaleProcess();
            ReadStatusCore();

            var psi = new ProcessStartInfo(ExecutablePath)
            {
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden,
                WorkingDirectory = DataDirectory,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
            };
            psi.ArgumentList.Add("--data-dir");
            psi.ArgumentList.Add(DataDirectory);
            psi.ArgumentList.Add("--parent-pid");
            psi.ArgumentList.Add(Environment.ProcessId.ToString());

            try
            {
                _process = Process.Start(psi);
                if (_process == null)
                {
                    AppLog.Warn("native WhatsApp bridge did not start");
                    return;
                }

                try { File.WriteAllText(PidPath, _process.Id.ToString()); } catch { /* diagnostics only */ }
                _process.OutputDataReceived += (_, e) => AppendLog("out", e.Data);
                _process.ErrorDataReceived += (_, e) => AppendLog("err", e.Data);
                _process.BeginOutputReadLine();
                _process.BeginErrorReadLine();

                _cts = new CancellationTokenSource();
                var token = _cts.Token;
                _ = Task.Run(() => MonitorAsync(token));
                AppLog.Info($"native WhatsApp bridge started (pid {_process.Id})");
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, "starting native WhatsApp bridge");
                _process = null;
            }
        }
    }

    public void Stop()
    {
        CancellationTokenSource? cts;
        Process? process;
        lock (_gate)
        {
            cts = _cts;
            _cts = null;
            process = _process;
            _process = null;
        }

        try { cts?.Cancel(); } catch { /* best effort */ }
        cts?.Dispose();

        try
        {
            if (process is { HasExited: false })
            {
                process.Kill(entireProcessTree: true);
                process.WaitForExit(2500);
            }
        }
        catch { /* already gone */ }

        try { process?.Dispose(); } catch { /* best effort */ }
        try { if (File.Exists(PidPath)) File.Delete(PidPath); } catch { /* best effort */ }
    }

    /// <summary>Stops the helper and removes the linked device and its local session.</summary>
    public void Forget()
    {
        Stop();
        try
        {
            if (Directory.Exists(DataDirectory)) Directory.Delete(DataDirectory, recursive: true);
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, "deleting native WhatsApp session");
        }

        lock (_gate) _status = new NativeWhatsAppStatus();
        StatusChanged?.Invoke();
    }

    private async Task MonitorAsync(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            try
            {
                ReadStatusCore();
                ReadEventsCore();
                lock (_gate)
                {
                    if (_process is { HasExited: true })
                    {
                        _status = new NativeWhatsAppStatus
                        {
                            State = "stopped",
                            Paired = _status.Paired,
                            UpdatedAt = DateTimeOffset.Now,
                            Error = "bridge process exited",
                        };
                        StatusChanged?.Invoke();
                        break;
                    }
                }
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, "native WhatsApp monitor");
            }

            try
            {
                await Task.Delay(700, token);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    private void ReadStatusCore()
    {
        if (!File.Exists(StatusPath)) return;
        NativeWhatsAppStatus? next;
        try
        {
            var json = File.ReadAllText(StatusPath);
            next = JsonSerializer.Deserialize<NativeWhatsAppStatus>(json, JsonOptions);
        }
        catch (Exception ex) when (ex is IOException or JsonException)
        {
            return;
        }
        if (next == null || string.IsNullOrWhiteSpace(next.State)) return;

        var changed = false;
        lock (_gate)
        {
            changed = !string.Equals(_status.State, next.State, StringComparison.OrdinalIgnoreCase) ||
                      _status.Paired != next.Paired ||
                      !string.Equals(_status.Error, next.Error, StringComparison.Ordinal) ||
                      !string.Equals(_status.Me, next.Me, StringComparison.Ordinal);
            _status = next;
        }

        if (changed) StatusChanged?.Invoke();
    }

    private void ReadEventsCore()
    {
        if (!File.Exists(EventsPath)) return;

        byte[] bytes;
        long length;
        using (var fs = new FileStream(EventsPath, FileMode.Open, FileAccess.Read,
                   FileShare.ReadWrite | FileShare.Delete))
        {
            length = fs.Length;
            if (length < _eventsOffset)
            {
                _eventsOffset = 0;
                _pending.Clear();
            }
            if (length == _eventsOffset) return;

            var count = checked((int)(length - _eventsOffset));
            bytes = new byte[count];
            fs.Seek(_eventsOffset, SeekOrigin.Begin);
            var read = 0;
            while (read < count)
            {
                var n = fs.Read(bytes, read, count - read);
                if (n <= 0) break;
                read += n;
            }
            if (read == 0) return;
            if (read != count) Array.Resize(ref bytes, read);
            _eventsOffset += read;
        }

        _pending.Append(Encoding.UTF8.GetString(bytes));
        while (true)
        {
            var text = _pending.ToString();
            var newline = text.IndexOf('\n');
            if (newline < 0) break;
            var line = text[..newline].TrimEnd('\r');
            _pending.Remove(0, newline + 1);
            if (string.IsNullOrWhiteSpace(line)) continue;

            try
            {
                var message = JsonSerializer.Deserialize<NativeWhatsAppMessage>(line, JsonOptions);
                if (message == null || string.IsNullOrWhiteSpace(message.Title)) continue;
                if (message.At == default) message.At = DateTimeOffset.Now;
                MessageObserved?.Invoke(message);
            }
            catch (JsonException ex)
            {
                AppLog.Exception(ex, "parsing native WhatsApp event");
            }
        }

        // A stalled partial line must not grow without bound.
        if (_pending.Length > 64 * 1024) _pending.Clear();
    }

    private void KillStaleProcess()
    {
        try
        {
            if (!File.Exists(PidPath)) return;
            if (!int.TryParse(File.ReadAllText(PidPath).Trim(), out var pid) || pid <= 0) return;
            var stale = Process.GetProcessById(pid);
            if (string.Equals(stale.ProcessName, "United.WhatsAppBridge", StringComparison.OrdinalIgnoreCase))
                stale.Kill(entireProcessTree: true);
            stale.Dispose();
        }
        catch
        {
            // Stale pid files are expected after a crash.
        }
    }

    private void AppendLog(string channel, string? line)
    {
        if (string.IsNullOrWhiteSpace(line)) return;
        try
        {
            Directory.CreateDirectory(DataDirectory);
            File.AppendAllText(LogPath, $"{DateTimeOffset.Now:O} [{channel}] {line}{Environment.NewLine}");
        }
        catch
        {
            // Logging the helper must never break the app.
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        Stop();
    }
}
