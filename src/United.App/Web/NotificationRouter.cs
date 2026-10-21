using Microsoft.Web.WebView2.Core;
using United.App.Diagnostics;
using United.Core.Notifications;
using United.Core.Services;
using United.Core.Settings;
using United.Platform.Windows.Notifications;

namespace United.App.Web;

/// <summary>
/// Routes web notifications from services to native Windows toasts.
/// Per-service modes: Host (we raise the toast — deduped, one live toast per service),
/// Webview (WebView2 default display), Off.
/// </summary>
public sealed class NotificationRouter
{
    private readonly ToastService _toasts;
    private readonly Func<AppSettings> _settings;
    private readonly Func<string, NotificationMode> _modeOf;

    /// <summary>Returns true when the service is on screen (active + window visible) — suppresses redundant toasts.</summary>
    public Func<string, bool> IsServiceOnScreen { get; set; } = _ => false;

    /// <summary>Raised for every notification that is recorded in the unified inbox (all modes except Off).</summary>
    public event Action<ServiceDefinition, string, string, DateTimeOffset>? NotificationObserved;

    private readonly Dictionary<string, NotificationGate> _gates = new(StringComparer.OrdinalIgnoreCase);

    public NotificationRouter(ToastService toasts, Func<AppSettings> settings, Func<string, NotificationMode> modeOf)
    {
        _toasts = toasts;
        _settings = settings;
        _modeOf = modeOf;
    }

    public void OnWebNotification(ServiceDefinition definition, CoreWebView2NotificationReceivedEventArgs e)
    {
        try
        {
            var mode = _modeOf(definition.Id);
            AppLog.Info($"web notification received [{definition.Id}] mode={mode}");
            if (mode == NotificationMode.Off)
            {
                AppLog.Info($"web notification ignored [{definition.Id}] mode=Off");
                e.Handled = true;
                return;
            }

            var n = e.Notification;
            var title = string.IsNullOrWhiteSpace(n.Title) ? definition.DisplayName : n.Title!;
            var body = n.Body ?? "";
            var at = NotificationTime.Normalize(n.Timestamp, DateTimeOffset.Now);

            // Subscriber only posts to WPF Dispatcher; actual serialization stays
            // off WebView2's notification callback and cannot block Notification().
            NotificationObserved?.Invoke(definition, title, body, at);

            if (mode == NotificationMode.Webview)
            {
                return; // WebView2 default: show its own native notification
            }

            // Host mode: consume the web notification and raise our own toast so that
            // click routing, sound policy and deduplication stay under app control.
            e.Handled = true;

            if (IsServiceOnScreen(definition.Id))
            {
                return; // user is looking at this chat; the service shows its own in-page banner
            }

            var key = NotificationGate.KeyFor(title, body);

            var gate = GetGate(definition.Id);
            if (!gate.ShouldNotify(key, DateTimeOffset.UtcNow)) return;

            var sound = _settings().NotificationSound && !n.IsSilent;
            _toasts.Show(definition.Id, title, body, sound);
        }
        catch (Exception ex)
        {
            Diagnostics.AppLog.Exception(ex, $"notification routing for {definition.Id}");
        }
    }

    private NotificationGate GetGate(string serviceId)
    {
        if (!_gates.TryGetValue(serviceId, out var gate))
            _gates[serviceId] = gate = new NotificationGate();
        return gate;
    }
}
