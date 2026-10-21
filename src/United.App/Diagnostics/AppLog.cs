using System.Diagnostics;
using System.IO;
using System.Text;

namespace United.App.Diagnostics;

/// <summary>Minimal rolling file logger at %LOCALAPPDATA%\United\logs. No external dependency.</summary>
public static class AppLog
{
    private static readonly object Gate = new();
    private static readonly string Dir =
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "United", "logs");

    public static string Directory_ => Dir;

    public static void Info(string message) => Write("INFO", message, null);

    public static void Warn(string message) => Write("WARN", message, null);

    public static void Exception(Exception ex, string context) => Write("ERROR", context, ex);

    private static void Write(string level, string message, Exception? ex)
    {
        try
        {
            var sb = new StringBuilder();
            sb.Append(DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff"))
              .Append(' ').Append(level).Append(' ').Append(message);
            if (ex != null)
            {
                sb.Append(" | ").Append(ex.GetType().Name).Append(": ").Append(ex.Message);
                for (var inner = ex.InnerException; inner != null; inner = inner.InnerException)
                    sb.AppendLine().Append("  -> caused by ").Append(inner.GetType().Name).Append(": ").Append(inner.Message);
                if (ex.StackTrace != null) sb.AppendLine().Append(ex.StackTrace);
            }
            sb.AppendLine();

            lock (Gate)
            {
                Directory.CreateDirectory(Dir);
                var file = Path.Combine(Dir, $"united-{DateTime.Now:yyyyMMdd}.log");
                File.AppendAllText(file, sb.ToString());
            }

            if (Debugger.IsAttached) Debug.Write(sb.ToString());
        }
        catch
        {
            // Logging must never crash the app.
        }
    }
}
