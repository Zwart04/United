namespace United.Core.Settings;

/// <summary>
/// Persisted application settings. Contains no credentials, tokens, or cookies —
/// authentication data lives exclusively inside per-service WebView2 profiles.
/// </summary>
public sealed class AppSettings
{
    /// <summary>system | light | dark</summary>
    public string Theme { get; set; } = "system";

    public bool LaunchAtStartup { get; set; }

    /// <summary>When true, the close button hides the window to the tray instead of exiting.</summary>
    public bool CloseMinimizesToTray { get; set; } = true;

    /// <summary>Start hidden in the tray.</summary>
    public bool StartMinimized { get; set; }

    public bool ShowTrayBadge { get; set; } = true;

    /// <summary>Sound for toasts raised by the host (services keep their own in-page sound).</summary>
    public bool NotificationSound { get; set; } = true;

    /// <summary>Ordered ids of services that appear in the sidebar.</summary>
    public List<string> EnabledServices { get; set; } = new()
    {
        "whatsapp", "instagram", "facebook", "telegram", "discord", "threads",
    };

    /// <summary>Schema version used by settings migrations (0 = before versioning).</summary>
    public int SettingsVersion { get; set; }

    /// <summary>Optional per-service NotificationMode overrides keyed by service id.</summary>
    public Dictionary<string, string> NotificationModes { get; set; } = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>Services the user manually hibernated (persisted across restarts).</summary>
    public List<string> HibernatedServices { get; set; } = new();

    /// <summary>Last active service id, restored on next launch.</summary>
    public string? LastActiveService { get; set; }

    /// <summary>Put non-focused services into low-memory mode automatically.</summary>
    public bool AutoLowMemory { get; set; } = true;

    /// <summary>
    /// Use the bundled native WhatsApp bridge for background notifications. The
    /// WhatsApp WebView is only kept asleep after the bridge has a linked device.
    /// </summary>
    public bool NativeWhatsAppEnabled { get; set; } = true;

    /// <summary>Check GitHub Releases for a newer installer after startup.</summary>
    public bool CheckForUpdatesOnStartup { get; set; } = true;

    /// <summary>Last successful automatic update check (UTC), used for daily throttling.</summary>
    public DateTimeOffset? LastUpdateCheckUtc { get; set; }

    /// <summary>Window geometry (persisted for a native feel across restarts).</summary>
    public double WindowLeft { get; set; } = double.NaN;
    public double WindowTop { get; set; } = double.NaN;
    public double WindowWidth { get; set; } = double.NaN;
    public double WindowHeight { get; set; } = double.NaN;
}
