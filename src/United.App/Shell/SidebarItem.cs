using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Media;
using United.App.Diagnostics;
using United.App.Web;
using United.Core.Services;

namespace United.App.Shell;

/// <summary>Bindable sidebar entry for one service.</summary>
public sealed class SidebarItem : INotifyPropertyChanged
{
    private int _unread;
    private bool _isActive;
    private bool _dormant; // hibernated or not yet created on purpose

    public SidebarItem(ServiceSlot slot, Geometry icon)
    {
        Slot = slot;
        Icon = icon;
        RefreshIcon();
    }

    public ServiceSlot Slot { get; }
    public Geometry Icon { get; }

    private ImageSource? _iconImage;

    /// <summary>Custom logo from IconStore, then the bundled Brand.* vector.
    /// Null means the sidebar falls back to the monochrome glyph.</summary>
    public ImageSource? IconImage
    {
        get => _iconImage;
        private set
        {
            if (ReferenceEquals(_iconImage, value)) return;
            _iconImage = value;
            Raise();
        }
    }

    /// <summary>Re-reads the logo after the user picks or resets one in Settings.</summary>
    public void RefreshIcon()
    {
        IconImage = IconStore.Resolve(Def);
        var source = IconImage == null ? "glyph fallback"
            : IconStore.FindOverride(Def.Id) != null ? "custom image"
            : "brand vector";
        AppLog.Info($"icon [{Def.Id}] -> {source}");
    }

    public ServiceDefinition Def => Slot.Def;
    public string Id => Def.Id;

    public string BadgeText => _unread < 0 ? "•" : _unread > 99 ? "9+" : _unread.ToString();
    public bool BadgeVisible => _unread != 0;

    public int Unread
    {
        get => _unread;
        set { if (_unread != value) { _unread = value; Raise(nameof(Unread)); Raise(nameof(BadgeText)); Raise(nameof(BadgeVisible)); } }
    }

    public bool IsActive
    {
        get => _isActive;
        set { if (_isActive != value) { _isActive = value; Raise(nameof(IsActive)); Raise(nameof(ToolTipText)); } }
    }

    public bool IsDormant
    {
        get => _dormant;
        set { if (_dormant != value) { _dormant = value; Raise(nameof(IsDormant)); Raise(nameof(ToolTipText)); } }
    }

    public string ToolTipText
    {
        get
        {
            var baseText = Def.DisplayName;
            if (IsDormant) return baseText + " — hibernated, click to resume";
            if (_unread > 0) return $"{baseText} — {_unread} unread";
            if (_unread < 0) return baseText + " — unread";
            return baseText;
        }
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    private void Raise([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
