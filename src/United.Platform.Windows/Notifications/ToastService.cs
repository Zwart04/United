using System.Runtime.InteropServices;
using System.Security;
using System.Text;
using Windows.Data.Xml.Dom;
using Windows.UI.Notifications;
using United.Platform.Windows.Activation;

namespace United.Platform.Windows.Notifications;

/// <summary>
/// Native Windows toast notifications for an unpackaged (classic Win32/WPF) app.
/// Uses raw WinRT toast APIs attributed to the app's AppUserModelID shortcut, with
/// protocol activation (united://open/{service}) for click routing — no COM activator
/// and no third-party notification library required.
/// </summary>
public sealed class ToastService : IDisposable
{
    private readonly ToastNotifier? _notifier;
    private readonly bool _shortcutOk;

    public event Action<string>? ToastFailed; // diagnostic hook: error message

    public ToastService(bool shortcutReady)
    {
        _shortcutOk = shortcutReady;
        try
        {
            _notifier = ToastNotificationManager.CreateToastNotifier(StartMenuShortcut.Aumid);
        }
        catch (Exception ex)
        {
            // No AUMID shortcut resolved yet — surface as diagnostic, never crash the app.
            _notifier = null;
            ToastFailed?.Invoke($"toast notifier unavailable: {ex.Message}");
        }
    }

    public bool ShortcutReady => _shortcutOk;

    public void Show(string serviceId, string title, string body, bool sound)
    {
        if (_notifier == null) return;
        if (string.IsNullOrWhiteSpace(title)) title = "United";
        var launchUri = ProtocolRegistration.BuildOpenUri(serviceId);

        var xml = new StringBuilder();
        xml.Append("<toast activationType=\"protocol\" launch=\"").Append(SecurityElement.Escape(launchUri)).Append("\">");
        xml.Append("<visual><binding template=\"ToastGeneric\">");
        xml.Append("<text>").Append(SecurityElement.Escape(Truncate(title, 120))).Append("</text>");
        if (!string.IsNullOrWhiteSpace(body))
            xml.Append("<text>").Append(SecurityElement.Escape(Truncate(body, 256))).Append("</text>");
        xml.Append("</binding></visual>");
        xml.Append(sound
            ? "<audio src=\"ms-winsoundevent:Notification.Default\"/>"
            : "<audio silent=\"true\"/>");
        xml.Append("</toast>");

        try
        {
            var doc = new XmlDocument();
            doc.LoadXml(xml.ToString());
            var toast = new ToastNotification(doc)
            {
                // One live toast per service — a newer message replaces the previous one
                // instead of stacking duplicates in Action Center.
                Tag = SanitizeTag(serviceId),
                Group = "United",
            };
            _notifier.Show(toast);
        }
        catch (Exception ex) when (ex is COMException or UnauthorizedAccessException or InvalidOperationException)
        {
            ToastFailed?.Invoke(ex.Message);
        }
    }

    private static string SanitizeTag(string value)
    {
        var sb = new StringBuilder(value.Length);
        foreach (var c in value)
            if (char.IsLetterOrDigit(c) || c is '-' or '_' or '.') sb.Append(c);
        return sb.Length > 0 ? sb.ToString() : "united";
    }

    private static string Truncate(string s, int max)
        => s.Length <= max ? s : s[..(max - 1)] + "…";

    public void Dispose()
    {
        // ToastNotifier holds no unmanaged resources requiring explicit cleanup.
    }
}
