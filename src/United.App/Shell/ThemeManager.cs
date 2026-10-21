using System.Windows;
using United.Platform.Windows.SystemThemes;

namespace United.App.Shell;

/// <summary>Swaps the theme resource dictionary and notifies listeners (title bar, webviews).</summary>
public sealed class ThemeManager
{
    private const int ThemeDictionaryIndex = 0;

    public AppTheme Current { get; private set; } = AppTheme.Light;

    public event Action<AppTheme>? ThemeChanged;

    /// <summary>Applies "system | light | dark". For system, reads the Windows app theme.</summary>
    public void Apply(string mode, AppTheme? systemTheme = null)
    {
        var resolved = mode?.Trim().ToLowerInvariant() switch
        {
            "dark" => AppTheme.Dark,
            "light" => AppTheme.Light,
            _ => systemTheme ?? ThemeWatcher.ReadSystemTheme(),
        };
        ApplyResolved(resolved);
    }

    public void ApplyResolved(AppTheme theme)
    {
        if (theme == Current && Application.Current.Resources.MergedDictionaries.Count > ThemeDictionaryIndex)
        {
            // Ensure the right dictionary is loaded even on first call.
            var current = Application.Current.Resources.MergedDictionaries[ThemeDictionaryIndex].Source?.OriginalString ?? "";
            if (theme == AppTheme.Dark && current.Contains("Dark")) return;
            if (theme == AppTheme.Light && current.Contains("Light")) return;
        }

        var dict = new ResourceDictionary
        {
            Source = new Uri($"Themes/{(theme == AppTheme.Dark ? "Dark" : "Light")}.xaml", UriKind.Relative),
        };

        var dicts = Application.Current.Resources.MergedDictionaries;
        if (dicts.Count > ThemeDictionaryIndex) dicts[ThemeDictionaryIndex] = dict;
        else dicts.Insert(ThemeDictionaryIndex, dict);

        Current = theme;
        ThemeChanged?.Invoke(theme);
    }
}
