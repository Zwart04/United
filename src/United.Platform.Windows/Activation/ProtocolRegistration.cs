using Microsoft.Win32;

namespace United.Platform.Windows.Activation;

/// <summary>
/// Registers the custom <c>united://</c> URL protocol under HKCU (no admin rights required).
/// Toast notifications use protocol activation (activationType="protocol"), which routes
/// clicks through this scheme — no COM activator registration needed.
/// </summary>
public static class ProtocolRegistration
{
    public const string Scheme = "united";

    public static bool EnsureRegistered(string exePath)
    {
        try
        {
            using var key = Registry.CurrentUser.CreateSubKey($@"Software\Classes\{Scheme}");
            if (key == null) return false;
            key.SetValue("", "URL:United Messenger Protocol");
            key.SetValue("URL Protocol", "");
            using (var cmd = key.CreateSubKey(@"shell\open\command"))
            {
                cmd?.SetValue("", $"\"{exePath}\" \"%1\"");
            }
            using (var icon = key.CreateSubKey(@"DefaultIcon"))
            {
                icon?.SetValue("", $"\"{exePath}\",0");
            }
            return true;
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
        {
            return false;
        }
    }

    public static bool IsRegistered
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey($@"Software\Classes\{Scheme}");
            return key?.GetValue("") != null;
        }
    }

    /// <summary>Parses "united://open/{serviceId}" (case-insensitive). Returns serviceId or null.</summary>
    public static string? ParseOpenRequest(string uri)
    {
        if (!Uri.TryCreate(uri, UriKind.Absolute, out var u) ||
            !string.Equals(u.Scheme, Scheme, StringComparison.OrdinalIgnoreCase))
            return null;

        // united://open/whatsapp → Host = "open", AbsolutePath = "/whatsapp"
        var parts = (u.Host + u.AbsolutePath).Split('/', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length >= 2 && string.Equals(parts[0], "open", StringComparison.OrdinalIgnoreCase))
            return parts[1];
        return null;
    }

    public static string BuildOpenUri(string serviceId) => $"{Scheme}://open/{serviceId.ToLowerInvariant()}";
}
