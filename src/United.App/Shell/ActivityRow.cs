using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows;
using System.Windows.Media;
using United.Core.Activity;

namespace United.App.Shell;

/// <summary>UI projection of one <see cref="ActivityEntry"/> with a resolved service logo.</summary>
public sealed class ActivityRow : INotifyPropertyChanged
{
    private ActivityEntry _entry;

    public ActivityRow(ActivityEntry entry)
    {
        _entry = entry;
        Icon = IconStore.LoadOverride(entry.ServiceId) ?? IconStore.ResolveBrand(entry.IconKey);
        Glyph = ResolveGlyph(entry.IconKey);
    }

    public ActivityEntry Entry => _entry;
    public string Key => KeyFor(_entry);

    /// <summary>
    /// Stable row identity: the conversation href when the service provides one,
    /// otherwise the display title. Telegram previews change every message, so a
    /// title-only key made the inbox drop/re-add the open conversation constantly.
    /// </summary>
    public static string KeyFor(ActivityEntry entry) =>
        entry.ServiceId + "\u001f" +
        (string.IsNullOrWhiteSpace(entry.Target) ? entry.Title : entry.Target);

    public ImageSource? Icon { get; }
    public Geometry? Glyph { get; }
    public bool HasIcon => Icon != null;

    public string Title => _entry.Title;
    public string ServiceName => string.IsNullOrWhiteSpace(_entry.ServiceName) ? _entry.ServiceId : _entry.ServiceName;
    public string Body => string.IsNullOrWhiteSpace(_entry.Body) ? "No preview" : _entry.Body;
    public string TimeText => FormatTime(_entry.LastAt);
    public string CountText => _entry.Count > 99 ? "99+" : _entry.Count.ToString();
    public bool HasCount => _entry.Count > 0;

    public void Update(ActivityEntry entry)
    {
        _entry = entry;
        Raise(nameof(Entry));
        Raise(nameof(Title));
        Raise(nameof(ServiceName));
        Raise(nameof(Body));
        Raise(nameof(TimeText));
        Raise(nameof(CountText));
        Raise(nameof(HasCount));
    }

    private static Geometry? ResolveGlyph(string iconKey)
    {
        var pascal = string.IsNullOrEmpty(iconKey)
            ? "Globe"
            : char.ToUpperInvariant(iconKey[0]) + iconKey[1..];
        return Application.Current?.TryFindResource("Icon." + pascal) as Geometry
               ?? Application.Current?.TryFindResource("Icon.Globe") as Geometry;
    }

    private static string FormatTime(DateTimeOffset at)
    {
        var local = at.ToLocalTime();
        var now = DateTimeOffset.Now;
        var day = local.Date;
        var today = now.Date;

        if (day == today)
        {
            var age = now - local;
            if (age.TotalMinutes < 1) return "now";
            return local.ToString("HH:mm");
        }

        if (day == today.AddDays(-1)) return "Yesterday";
        if (day > today.AddDays(-7)) return local.ToString("ddd");
        if (day.Year == today.Year) return local.ToString("dd MMM");
        return local.ToString("dd MMM yyyy");
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    private void Raise([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
