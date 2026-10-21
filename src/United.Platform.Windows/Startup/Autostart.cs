using Microsoft.Win32;

namespace United.Platform.Windows.Startup;

/// <summary>Per-user "run at logon" via HKCU\...\CurrentVersion\Run (no admin required).</summary>
public static class Autostart
{
    private const string ValueName = "United Messenger";
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";

    public static bool IsEnabled
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue(ValueName) != null;
        }
    }

    public static bool SetEnabled(bool enabled, string exePath, bool startMinimized)
    {
        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(RunKey);
            if (key == null) return false;
            if (enabled)
            {
                var cmd = $"\"{exePath}\"";
                if (startMinimized) cmd += " --minimized";
                key.SetValue(ValueName, cmd);
            }
            else
            {
                key.DeleteValue(ValueName, throwOnMissingValue: false);
            }
            return true;
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
        {
            return false;
        }
    }
}
