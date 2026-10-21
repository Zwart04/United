namespace United.Core.Services;

/// <summary>How notifications for a service are surfaced to Windows.</summary>
public enum NotificationMode
{
    /// <summary>Host intercepts WebView2 NotificationReceived events and raises native toasts itself.</summary>
    Host = 0,

    /// <summary>Let WebView2's default handling display the native notification.</summary>
    Webview = 1,

    /// <summary>Suppress notifications for this service.</summary>
    Off = 2,
}

/// <summary>
/// Declarative description of a single web service hosted inside the app.
/// All per-service quirks live here (or in the user override file), never in code paths.
/// </summary>
public sealed class ServiceDefinition
{
    public string Id { get; init; } = "";

    public string DisplayName { get; init; } = "";

    /// <summary>Initial URL opened when the service is created.</summary>
    public string Url { get; init; } = "";

    /// <summary>Icon key resolved against the app icon dictionary.</summary>
    public string Icon { get; init; } = "globe";

    /// <summary>
    /// Origin hosts that belong to this service. Used for permission grants and to decide
    /// whether a navigation stays in-app or goes to the system browser.
    /// </summary>
    public string[] OriginHosts { get; init; } = Array.Empty<string>();

    /// <summary>Hosts that must always open in the system default browser.</summary>
    public string[] ExternalHosts { get; init; } = Array.Empty<string>();

    /// <summary>Hosts that must stay in-app (OAuth/popups) even though they are cross-origin.</summary>
    public string[] AuthPopupHosts { get; init; } = Array.Empty<string>();

    /// <summary>
    /// Ordered regular expressions tested against document.title.
    /// A match with a "count" capture group yields an exact unread number,
    /// a match without it yields an unknown-count unread dot.
    /// </summary>
    public string[] TitlePatterns { get; init; } = Array.Empty<string>();

    public NotificationMode NotificationMode { get; init; } = NotificationMode.Host;

    public bool EnabledByDefault { get; init; }

    /// <summary>Optional User-Agent override (Settings.UserAgent). Null keeps the WebView2 default.</summary>
    public string? UserAgent { get; init; }

    /// <summary>
    /// Some web apps (Telegram Web K) only build their chat list while their surface is
    /// actually rendered. Such a slot keeps its WebView2 visible but parked outside the
    /// pane when another service is active, so background observation still works.
    /// </summary>
    public bool KeepRenderingWhenHidden { get; init; }

    /// <summary>Whether this service is a candidate for manual hibernation in the UI.</summary>
    public bool SupportsHibernation { get; init; } = true;

    public bool IsValid()
    {
        if (string.IsNullOrWhiteSpace(Id) || string.IsNullOrWhiteSpace(Url)) return false;
        foreach (var bad in new[] { '/', '\\', ':', '*', '?', '"', '<', '>', '|' })
            if (Id.Contains(bad)) return false;
        return Uri.TryCreate(Url, UriKind.Absolute, out var uri) &&
               (uri.Scheme == Uri.UriSchemeHttps || uri.Scheme == Uri.UriSchemeHttp);
    }
}
