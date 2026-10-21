using System.Text.Json;

namespace United.Core.Services;

/// <summary>
/// Built-in service catalog plus resolution of user-enabled services.
/// Adding a service (Threads, X, Reddit, ...) is data, not code.
/// </summary>
public static class ServiceCatalog
{
    /// <summary>Services shown by default (sidebar order).</summary>
    public static readonly IReadOnlyList<ServiceDefinition> BuiltIn = new[]
    {
        new ServiceDefinition
        {
            Id = "whatsapp",
            DisplayName = "WhatsApp",
            Url = "https://web.whatsapp.com/",
            Icon = "whatsapp",
            OriginHosts = new[] { "web.whatsapp.com", "whatsapp.com" },
            ExternalHosts = new[] { "whatsapp.com" },
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            NotificationMode = NotificationMode.Host,
            EnabledByDefault = true,
        },
        new ServiceDefinition
        {
            Id = "telegram",
            DisplayName = "Telegram",
            Url = "https://web.telegram.org/k/",
            Icon = "telegram",
            OriginHosts = new[] { "web.telegram.org", "telegram.org" },
            ExternalHosts = new[] { "telegram.org" },
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            NotificationMode = NotificationMode.Host,
            EnabledByDefault = true,
            // Web K builds its chat list only while the surface is rendered, so the slot
            // stays visible (parked outside the pane) even when another service is active.
            KeepRenderingWhenHidden = true,
        },
        new ServiceDefinition
        {
            Id = "instagram",
            DisplayName = "Instagram",
            Url = "https://www.instagram.com/direct/inbox/",
            Icon = "instagram",
            OriginHosts = new[] { "instagram.com", "cdninstagram.com" },
            ExternalHosts = new[] { "cdninstagram.com" },
            AuthPopupHosts = new[] { "facebook.com" },
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            NotificationMode = NotificationMode.Host,
            EnabledByDefault = true,
        },
        new ServiceDefinition
        {
            Id = "facebook",
            DisplayName = "Facebook",
            Url = "https://www.facebook.com/messages/t/",
            Icon = "facebook",
            OriginHosts = new[] { "facebook.com", "messenger.com", "fbcdn.net", "facebook.net" },
            ExternalHosts = Array.Empty<string>(),
            AuthPopupHosts = new[] { "facebook.com" },
            // Messenger caps the title badge at "(20+)", so the trailing plus is allowed.
            TitlePatterns = new[] { @"^\((?<count>\d+)\+?\)" },
            NotificationMode = NotificationMode.Host,
            EnabledByDefault = true,
        },
        new ServiceDefinition
        {
            Id = "discord",
            DisplayName = "Discord",
            Url = "https://discord.com/app",
            Icon = "discord",
            OriginHosts = new[] { "discord.com", "discordapp.com", "discord.gg" },
            ExternalHosts = new[] { "discord.gg" },
            // Discord's tab title counts mentions across all servers, not DMs. The
            // sidebar badge comes from the Direct Messages icon via the observer.
            TitlePatterns = Array.Empty<string>(),
            NotificationMode = NotificationMode.Host,
            EnabledByDefault = true,
        },
        new ServiceDefinition
        {
            Id = "threads",
            DisplayName = "Threads",
            Url = "https://www.threads.com/messages/",
            Icon = "threads",
            OriginHosts = new[] { "threads.net", "threads.com" },
            ExternalHosts = new[] { "threads.com" },
            AuthPopupHosts = new[] { "instagram.com", "facebook.com" },
            // Threads web prefixes the tab title with "(n)" while DMs are unread.
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            NotificationMode = NotificationMode.Host,
            EnabledByDefault = true,
        },
    };

    /// <summary>Optional extras the user can enable with one click in Settings.</summary>
    public static readonly IReadOnlyList<ServiceDefinition> Extras = new[]
    {
        new ServiceDefinition
        {
            Id = "x",
            DisplayName = "X",
            Url = "https://x.com/messages",
            Icon = "x",
            OriginHosts = new[] { "x.com", "twitter.com" },
            ExternalHosts = Array.Empty<string>(),
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            EnabledByDefault = false,
        },
        new ServiceDefinition
        {
            Id = "messenger",
            DisplayName = "Messenger",
            Url = "https://www.messenger.com/",
            Icon = "messenger",
            OriginHosts = new[] { "messenger.com", "facebook.com" },
            ExternalHosts = new[] { "facebook.com" },
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            EnabledByDefault = false,
        },
        new ServiceDefinition
        {
            Id = "reddit",
            DisplayName = "Reddit",
            Url = "https://www.reddit.com/messages",
            Icon = "reddit",
            OriginHosts = new[] { "reddit.com" },
            ExternalHosts = Array.Empty<string>(),
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            EnabledByDefault = false,
        },
        new ServiceDefinition
        {
            Id = "linkedin",
            DisplayName = "LinkedIn",
            Url = "https://www.linkedin.com/messaging/",
            Icon = "linkedin",
            OriginHosts = new[] { "linkedin.com" },
            ExternalHosts = Array.Empty<string>(),
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            EnabledByDefault = false,
        },
        new ServiceDefinition
        {
            Id = "bluesky",
            DisplayName = "Bluesky",
            Url = "https://bsky.app/",
            Icon = "bluesky",
            OriginHosts = new[] { "bsky.app" },
            ExternalHosts = Array.Empty<string>(),
            TitlePatterns = Array.Empty<string>(),
            EnabledByDefault = false,
        },
        new ServiceDefinition
        {
            Id = "youtube",
            DisplayName = "YouTube",
            Url = "https://www.youtube.com/messages",
            Icon = "youtube",
            OriginHosts = new[] { "youtube.com" },
            ExternalHosts = Array.Empty<string>(),
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            EnabledByDefault = false,
        },
    };

    public static IReadOnlyList<ServiceDefinition> AllBuiltIn => BuiltIn.Concat(Extras).ToArray();

    public static ServiceDefinition? Find(string id)
    {
        foreach (var s in AllBuiltIn)
            if (string.Equals(s.Id, id, StringComparison.OrdinalIgnoreCase))
                return s;
        return null;
    }

    /// <summary>
    /// Merge built-in catalog, user-defined services (raw JSON array) and the enabled set
    /// from settings into the final ordered list of services to host.
    /// </summary>
    public static List<ServiceDefinition> Resolve(IEnumerable<string> enabledIds, string? userServicesJson, out List<string> problems)
    {
        problems = new List<string>();
        var byId = new Dictionary<string, ServiceDefinition>(StringComparer.OrdinalIgnoreCase);
        foreach (var s in AllBuiltIn) byId[s.Id] = s;

        if (!string.IsNullOrWhiteSpace(userServicesJson))
        {
            try
            {
                var user = JsonSerializer.Deserialize<List<ServiceDefinition>>(
                    userServicesJson,
                    new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
                if (user != null)
                {
                    foreach (var s in user)
                    {
                        if (!s.IsValid()) { problems.Add($"Invalid custom service '{s.Id}' ignored."); continue; }
                        if (byId.ContainsKey(s.Id)) { problems.Add($"Custom service '{s.Id}' overrides a built-in definition."); }
                        byId[s.Id] = s;
                    }
                }
            }
            catch (JsonException ex)
            {
                problems.Add($"services.json could not be parsed: {ex.Message}");
            }
        }

        var result = new List<ServiceDefinition>();
        foreach (var id in enabledIds)
        {
            if (byId.TryGetValue(id, out var def)) result.Add(def);
            else problems.Add($"Enabled service '{id}' has no definition.");
        }

        // Any custom (non-built-in) definition that is enabled by default joins at the end.
        foreach (var s in byId.Values)
            if (s.EnabledByDefault && !enabledIds.Contains(s.Id, StringComparer.OrdinalIgnoreCase) &&
                Find(s.Id) == null)
                result.Add(s);

        return result;
    }
}
