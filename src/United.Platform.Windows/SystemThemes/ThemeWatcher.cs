using Microsoft.Win32;

namespace United.Platform.Windows.SystemThemes;

public enum AppTheme
{
    Light,
    Dark,
}

/// <summary>Reads Windows app theme (AppsUseLightTheme) and raises change notifications.</summary>
public sealed class ThemeWatcher : IDisposable
{
    private bool _disposed;

    public event Action<AppTheme>? ThemeChanged;

    public AppTheme Current => ReadSystemTheme();

    public static AppTheme ReadSystemTheme()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            var value = key?.GetValue("AppsUseLightTheme");
            if (value is int i) return i == 0 ? AppTheme.Dark : AppTheme.Light;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // fall through to default
        }
        return AppTheme.Light;
    }

    public ThemeWatcher()
    {
        // UserPreferenceChanged fires for most personalization changes on Win10.
        // The main window additionally re-checks on WM_SETTINGCHANGE (ImmersiveColorSet).
        SystemEvents.UserPreferenceChanged += OnUserPreferenceChanged;
    }

    private void OnUserPreferenceChanged(object sender, UserPreferenceChangedEventArgs e)
    {
        if (e.Category is UserPreferenceCategory.General or UserPreferenceCategory.VisualStyle or UserPreferenceCategory.Color)
            Notify();
    }

    public void Notify()
    {
        if (_disposed) return;
        try
        {
            ThemeChanged?.Invoke(Current);
        }
        catch
        {
            // never let a listener exception kill the watcher
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        SystemEvents.UserPreferenceChanged -= OnUserPreferenceChanged;
    }
}
