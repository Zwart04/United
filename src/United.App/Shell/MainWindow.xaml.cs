using System.Collections.ObjectModel;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using Microsoft.Web.WebView2.Core;
using Microsoft.Win32;
using United.App.Diagnostics;
using United.App.Services;
using United.App.Views;
using United.App.Web;
using United.Core.Activity;
using United.Core.Lifecycle;
using United.Core.Notifications;
using United.Core.ReadState;
using United.Core.Services;
using United.Core.Settings;
using United.Platform.Windows.Interop;
using United.Platform.Windows.Metrics;
using United.Platform.Windows.Notifications;
using United.Platform.Windows.SystemThemes;

namespace United.App.Shell;

public partial class MainWindow : Window
{
    private readonly AppSettings _settings;
    private readonly SettingsStore _store;
    private readonly ThemeManager _theme;
    private readonly ThemeWatcher _watcher;
    private readonly ProcessMetricsCollector _metrics;
    private readonly NotificationRouter _router;
    private readonly ToastService _toasts;
    private readonly NativeWhatsAppBridge _nativeWhatsApp;
    private readonly NotificationGate _nativeWhatsAppGate = new();
    private readonly BadgeState _badges = new();
    private readonly ActivityStore _activity = new(ActivityStore.DefaultPath);
    private readonly ObservableCollection<ActivityRow> _activityRows = new();
    private readonly Dictionary<string, ActivityRow> _activityCache = new(StringComparer.OrdinalIgnoreCase);

    private readonly List<ServiceSlot> _slots = new();
    private readonly List<SidebarItem> _items = new();

    private string? _activeServiceId;
    private string? _activityChatServiceId;
    private string? _activitySelectedKey;
    private bool _initialized;
    private bool _exitRequested;
    private bool _showingActivity;
    private bool _suppressActivitySelection;
    private bool _rebuildingActivity;
    private bool _suppressAllChecked;
    private int _activityOpenToken;

    public event Action<int>? UnreadTotalChanged;

    public AppSettings Settings => _settings;
    public NativeWhatsAppBridge NativeWhatsApp => _nativeWhatsApp;
    public SettingsStore Store => _store;
    public ThemeManager Theme => _theme;
    public ThemeWatcher Watcher => _watcher;
    public ProcessMetricsCollector Metrics => _metrics;
    public IReadOnlyList<SidebarItem> Items => _items;

    private SettingsWindow? _settingsWindow;

    public MainWindow(AppSettings settings, SettingsStore store, ThemeManager theme,
        ThemeWatcher watcher, ProcessMetricsCollector metrics, Platform.Windows.Notifications.ToastService toasts)
    {
        _settings = settings;
        _store = store;
        _theme = theme;
        _watcher = watcher;
        _metrics = metrics;
        _toasts = toasts;

        _nativeWhatsApp = new NativeWhatsAppBridge(settings);
        _nativeWhatsApp.MessageObserved += OnNativeWhatsAppMessage;
        _nativeWhatsApp.StatusChanged += OnNativeWhatsAppStatusChanged;

        InitializeComponent();

        ActivityList.ItemsSource = _activityRows;
        _activity.Changed += () => Dispatcher.BeginInvoke(RefreshActivity);
        RefreshActivity();

        RestoreWindowGeometry();
        _theme.ThemeChanged += OnThemeChanged;

        _router = new NotificationRouter(
            toasts,
            () => _settings,
            ResolveNotificationMode)
        {
            IsServiceOnScreen = IsServiceOnScreen,
        };
        _router.NotificationObserved += OnNotificationObserved;

        _badges.Changed += () => Dispatcher.BeginInvoke(UpdateBadges);

        Activated += (_, _) => ApplyMemoryPolicy();
        Deactivated += (_, _) => ApplyMemoryPolicy();
        StateChanged += (_, _) => ApplyMemoryPolicy();
        SizeChanged += (_, _) =>
        {
            if (_showingActivity) ApplyWebVisibility();
        };
        SourceInitialized += (_, _) => ApplyDwmTheme();

        PreviewKeyDown += OnPreviewKeyDown;
    }

    // ── Initialization ────────────────────────────────────────────────

    public async Task InitializeAsync()
    {
        // Force the WPF window handle to exist before any WebView2 (HwndHost) is created —
        // otherwise initialization would wait for a window that is not shown yet.
        _ = new WindowInteropHelper(this).EnsureHandle();

        // Start the native helper before deciding whether WhatsApp can stay asleep.
        // It is harmless when unpaired: the normal WhatsApp WebView remains active.
        _nativeWhatsApp.Start();
        var nativeWhatsAppReady = _nativeWhatsApp.IsEnabled && _nativeWhatsApp.IsPaired;
        AppLog.Info($"native WhatsApp: available={_nativeWhatsApp.IsAvailable} paired={_nativeWhatsApp.IsPaired} state={_nativeWhatsApp.Status.State}");

        var userServicesJson = LoadUserServicesJson();
        var services = ServiceCatalog.Resolve(_settings.EnabledServices, userServicesJson, out var problems);
        foreach (var problem in problems) AppLog.Warn(problem);

        _initialized = true;

        if (services.Count == 0)
        {
            EmptyState.Visibility = Visibility.Visible;
            return;
        }

        var activeId = services.Any(s => s.Id == _settings.LastActiveService)
            ? _settings.LastActiveService!
            : services[0].Id;

        // Create the active service first for a fast first paint, then the rest staggered
        // so four simultaneous initializations don't compete for CPU on startup.
        // A hibernated service must stay asleep on startup too — otherwise "hibernate all"
        // would silently wake the last active service and defeat the memory saving.
        var active = services.First(s => s.Id == activeId);
        var activeDormant = ShouldStartDormant(active, nativeWhatsAppReady);
        AppLog.Info($"hibernation at startup: {_settings.HibernatedServices.Count} asleep (active={activeId}, dormant={activeDormant}, nativeWhatsApp={nativeWhatsAppReady})");
        AddService(active, dormant: activeDormant);
        foreach (var def in services.Where(s => s.Id != activeId))
        {
            var dormant = ShouldStartDormant(def, nativeWhatsAppReady);
            AddService(def, dormant);
            await Task.Delay(120);
        }

        ActivateService(activeId, resumeDormant: false);
        AppLog.Info($"initialized with {services.Count} service(s); active={activeId}");
    }

    private static string? LoadUserServicesJson()
    {
        try
        {
            var path = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "United", "services.json");
            return File.Exists(path) ? File.ReadAllText(path) : null;
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, "loading services.json");
            return null;
        }
    }

    private bool ShouldStartDormant(ServiceDefinition def, bool nativeWhatsAppReady)
    {
        // A paired native bridge owns WhatsApp while its WebView is asleep.
        // Native-induced sleep is never persisted, so a stored flag here is a
        // deliberate user choice and must survive a restart too.
        if (string.Equals(def.Id, "whatsapp", StringComparison.OrdinalIgnoreCase) && nativeWhatsAppReady)
            return true;
        return _settings.HibernatedServices.Contains(def.Id, StringComparer.OrdinalIgnoreCase);
    }

    private void AddService(ServiceDefinition def, bool dormant)
    {
        var slot = new ServiceSlot(def, ContentHost);
        slot.ApplyTheme(_theme.Current == AppTheme.Dark);
        var item = new SidebarItem(slot, LoadIcon(def.Icon)) { IsDormant = dormant };

        slot.TitleChanged += OnSlotTitleChanged;
        slot.NavigatingChanged += OnSlotNavigating;
        slot.StateChanged += OnSlotStateChanged;
        slot.Failed += (_, _) => Dispatcher.BeginInvoke(UpdateOverlay);
        slot.PopupRequested += OnSlotPopupRequested;
        slot.RecreateRequested += OnSlotRecreateRequested;
        slot.WebNotification += OnSlotWebNotification;
        slot.LiveActivityObserved += OnSlotLiveActivityObserved;
        slot.UnreadCountObserved += OnSlotUnreadCountObserved;

        _slots.Add(slot);
        _items.Add(item);
        SidebarList.Items.Add(item);

        if (!dormant)
        {
            AppLog.Info($"creating service webview: {def.Id}");
            _ = slot.CreateAsync();
        }
        else
        {
            AppLog.Info($"service asleep: {def.Id}");
        }
    }

    /// <summary>Settings: enable/disable services live.</summary>
    public void SetServiceEnabled(ServiceDefinition def, bool enabled)
    {
        if (enabled)
        {
            EnableService(def);
            return;
        }

        var existingIndex = _items.FindIndex(i => i.Id == def.Id);
        if (existingIndex >= 0)
        {
            var item = _items[existingIndex];
            item.Slot.Dispose();
            _slots.Remove(item.Slot);
            _items.RemoveAt(existingIndex);
            SidebarList.Items.RemoveAt(existingIndex);
            _badges.Clear(def.Id);
            _settings.EnabledServices.RemoveAll(id => string.Equals(id, def.Id, StringComparison.OrdinalIgnoreCase));
            if (_activeServiceId == def.Id)
            {
                _activeServiceId = null;
                if (_items.Count > 0) ActivateService(_items[0].Id);
                else
                {
                    ActiveServiceText.Text = "";
                    EmptyState.Visibility = Visibility.Visible;
                }
            }
        }
        _store.Save(_settings);
        RefreshActivity();
    }

    /// <summary>Adds a service to the sidebar and makes it active (used by Settings).</summary>
    public void EnableService(ServiceDefinition def)
    {
        if (_items.Any(i => string.Equals(i.Id, def.Id, StringComparison.OrdinalIgnoreCase))) return;

        if (!_settings.EnabledServices.Any(id => string.Equals(id, def.Id, StringComparison.OrdinalIgnoreCase)))
            _settings.EnabledServices.Add(def.Id);

        AddService(def, dormant: false);
        ActivateService(def.Id);
        _store.Save(_settings);
        RefreshActivity();
        AppLog.Info($"service enabled: {def.Id}");
    }

    /// <summary>Custom service created from the Add Service dialog; persisted to services.json.</summary>
    public ServiceDefinition AddCustomService(string displayName, string url, string? logoSourcePath, NotificationMode mode)
    {
        var id = MakeUniqueServiceId(displayName);
        var host = new Uri(url).Host;
        var def = new ServiceDefinition
        {
            Id = id,
            DisplayName = displayName.Trim(),
            Url = url,
            Icon = "globe",
            OriginHosts = host.StartsWith("www.", StringComparison.OrdinalIgnoreCase)
                ? new[] { host, host[4..] }
                : new[] { host, "www." + host },
            ExternalHosts = Array.Empty<string>(),
            TitlePatterns = new[] { @"^\((?<count>\d+)\)" },
            NotificationMode = mode,
            EnabledByDefault = false,
        };

        CustomServicesFile.AddOrReplace(def);
        if (!string.IsNullOrWhiteSpace(logoSourcePath))
            IconStore.SaveOverride(id, logoSourcePath);

        EnableService(def);
        AppLog.Info($"custom service added: {id} -> {url}");
        return def;
    }

    /// <summary>Removes a user-defined service from the sidebar and services.json.</summary>
    public void RemoveCustomService(string serviceId)
    {
        var def = _items.FirstOrDefault(i => string.Equals(i.Id, serviceId, StringComparison.OrdinalIgnoreCase))?.Def;
        if (def == null) return;
        if (ServiceCatalog.Find(def.Id) != null) return; // built-ins can only be disabled

        SetServiceEnabled(def, false);
        CustomServicesFile.Remove(def.Id);
        IconStore.DeleteOverride(def.Id);
        _settings.HibernatedServices.RemoveAll(id => string.Equals(id, def.Id, StringComparison.OrdinalIgnoreCase));
        _store.Save(_settings);
        AppLog.Info($"custom service removed: {def.Id}");
    }

    private string MakeUniqueServiceId(string displayName)
    {
        var slug = Slug(displayName);
        var taken = new HashSet<string>(_items.Select(i => i.Id), StringComparer.OrdinalIgnoreCase);
        foreach (var s in ServiceCatalog.AllBuiltIn) taken.Add(s.Id);
        foreach (var s in CustomServicesFile.Load()) taken.Add(s.Id);

        var candidate = slug;
        for (var n = 2; taken.Contains(candidate); n++) candidate = $"{slug}-{n}";
        return candidate;
    }

    private static string Slug(string name)
    {
        var sb = new StringBuilder();
        foreach (var ch in name.Trim().ToLowerInvariant())
        {
            if (char.IsLetterOrDigit(ch)) sb.Append(ch);
            else if (sb.Length > 0 && sb[^1] != '-') sb.Append('-');
        }

        var slug = sb.ToString().Trim('-');
        if (slug.Length == 0) slug = "service";
        if (slug.Length > 32) slug = slug[..32].TrimEnd('-');
        return slug;
    }

    // ── Logos ────────────────────────────────────────────────────────

    /// <summary>Opens the image picker and applies the chosen logo to a service.</summary>
    public bool PromptChangeServiceLogo(string serviceId, Window owner)
    {
        var dialog = new OpenFileDialog
        {
            Title = "Choose a service logo",
            Filter = IconStore.FileDialogFilter,
            CheckFileExists = true,
        };
        if (dialog.ShowDialog(owner) != true) return false;
        return ApplyServiceLogo(serviceId, dialog.FileName, owner);
    }

    public bool ApplyServiceLogo(string serviceId, string sourcePath, Window owner)
    {
        try
        {
            IconStore.SaveOverride(serviceId, sourcePath);
            RefreshServiceIcon(serviceId);
            return true;
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"change logo [{serviceId}] {sourcePath}");
            MessageBox.Show(owner,
                "That file could not be used as a logo.\n\n" +
                "Choose a PNG, JPG, ICO, BMP or GIF image (PNG recommended, square, 128-256 px).\n\n" +
                ex.Message,
                "United - logo", MessageBoxButton.OK, MessageBoxImage.Warning);
            return false;
        }
    }

    public void ResetServiceLogo(string serviceId)
    {
        IconStore.DeleteOverride(serviceId);
        RefreshServiceIcon(serviceId);
        AppLog.Info($"custom logo cleared [{serviceId}]");
    }

    private void RefreshServiceIcon(string serviceId)
    {
        // The unified inbox caches one resolved logo per conversation row.
        foreach (var key in _activityCache.Keys
                     .Where(k => k.StartsWith(serviceId + "\u001f", StringComparison.OrdinalIgnoreCase))
                     .ToList())
            _activityCache.Remove(key);

        var item = _items.FirstOrDefault(i => string.Equals(i.Id, serviceId, StringComparison.OrdinalIgnoreCase));
        if (item != null)
        {
            item.RefreshIcon();
            if (item.IsActive) SetOverlayIcon(item);
        }
        RefreshActivity();
    }

    public void ToggleHibernate(string serviceId)
    {
        var item = _items.FirstOrDefault(i => i.Id == serviceId);
        if (item == null) return;

        if (item.Slot.IsLive)
        {
            item.Slot.Hibernate();
            item.IsDormant = true;
        }
        else
        {
            item.IsDormant = false;
            if (item.IsActive) _ = ResumeAsync(item);
            else _ = item.Slot.CreateAsync();
        }
        _settings.HibernatedServices = _items.Where(i => i.IsDormant).Select(i => i.Id).ToList();
        _store.Save(_settings);
        UpdateOverlay();
        RefreshActivity();
    }

    public void HibernateAll()
    {
        foreach (var item in _items)
        {
            if (item.Slot.IsLive)
            {
                item.Slot.Hibernate();
                item.IsDormant = true;
            }
        }
        _settings.HibernatedServices = _items.Where(i => i.IsDormant).Select(i => i.Id).ToList();
        _store.Save(_settings);
        UpdateOverlay();
        RefreshActivity();
    }

    // ── Activation / switching ───────────────────────────────────────

    public void ActivateService(string serviceId, bool resumeDormant = true)
    {
        Dispatcher.BeginInvoke(() => ActivateServiceCore(serviceId, resumeDormant));
    }

    private void ActivateServiceCore(string serviceId, bool resumeDormant = true)
    {
        var item = _items.FirstOrDefault(i => i.Id == serviceId);
        if (item == null) return;

        _showingActivity = false;
        _activityChatServiceId = null;
        _activityOpenToken++;
        ActivityPanel.Visibility = Visibility.Collapsed;
        if (AllButton.IsChecked == true)
        {
            _suppressAllChecked = true;
            AllButton.IsChecked = false;
            _suppressAllChecked = false;
        }

        foreach (var i in _items) i.IsActive = i.Id == serviceId;
        _activeServiceId = serviceId;
        _settings.LastActiveService = serviceId;
        ActiveServiceText.Text = "· " + item.Def.DisplayName;

        if (item.IsDormant)
        {
            if (!resumeDormant)
            {
                // Startup on a hibernated service: keep it asleep and show the Resume overlay.
                UpdateOverlay();
                return;
            }

            item.IsDormant = false;
            // Resuming a service is a user decision: drop it from the persisted
            // hibernation list immediately so a crash or restart cannot put it
            // back to sleep against the user's intent.
            _settings.HibernatedServices.RemoveAll(id => string.Equals(id, serviceId, StringComparison.OrdinalIgnoreCase));
            _store.Save(_settings);
            _ = ResumeAsync(item);
        }
        else
        {
            ApplyMemoryPolicy();
        }
        UpdateOverlay();
    }

    private async Task ResumeAsync(SidebarItem item)
    {
        await item.Slot.CreateAsync();
        if (_exitRequested) return;
        _ = Dispatcher.BeginInvoke(() =>
        {
            if (_activeServiceId == item.Id)
            {
                ApplyMemoryPolicy();
            }
            UpdateOverlay();
        });
    }

    private SidebarItem? ActiveItem =>
        _showingActivity ? null : _items.FirstOrDefault(i => i.Id == _activeServiceId) ?? _items.FirstOrDefault();

    private ServiceSlot? ActiveSlot => ActiveItem?.Slot;

    // ── Slot event handlers ──────────────────────────────────────────

    private void OnSlotTitleChanged(ServiceSlot slot, string title)
        => Dispatcher.BeginInvoke(() =>
        {
            // A title without an unread count must not wipe a count read from the page's
            // own badges (Telegram Web K, for example, keeps the count in the chat list).
            var reading = UnreadParser.Parse(slot.Def, title);
            if (reading.Matched) _badges.Set(slot.Id, reading);
        });

    private void OnSlotUnreadCountObserved(ServiceSlot slot, int count)
        => Dispatcher.BeginInvoke(() => _badges.SetExact(slot.Id, count));

    private void OnSlotNavigating(ServiceSlot slot, bool loading)
    {
        if (slot.Id != _activeServiceId) return;
        Dispatcher.BeginInvoke(() => SetProgress(loading));
    }

    // IsVisibleChanged is a plain CLR event (not routed), so the indeterminate progress
    // bar animation is driven from code instead of an EventTrigger.
    private Storyboard? _progressStoryboard;

    private void SetProgress(bool loading)
    {
        if (loading)
        {
            _progressStoryboard ??= CreateProgressStoryboard();
            Progress.Visibility = Visibility.Visible;
            _progressStoryboard.Begin(this, true);
        }
        else
        {
            Progress.Visibility = Visibility.Collapsed;
            _progressStoryboard?.Stop(this);
        }
    }

    private Storyboard CreateProgressStoryboard()
    {
        var anim = new DoubleAnimation(-160, 1500, TimeSpan.FromMilliseconds(900))
        {
            RepeatBehavior = RepeatBehavior.Forever,
        };
        Storyboard.SetTarget(anim, Progress);
        Storyboard.SetTargetProperty(anim, new PropertyPath("(UIElement.RenderTransform).(TranslateTransform.X)"));
        var sb = new Storyboard();
        sb.Children.Add(anim);
        return sb;
    }

    private void OnSlotStateChanged(ServiceSlot slot)
        => Dispatcher.BeginInvoke(() =>
        {
            if (slot.Id == _activeServiceId) UpdateOverlay();
            ApplyMemoryPolicy();
        });

    private async void OnSlotPopupRequested(ServiceSlot slot, string uri)
    {
        try
        {
            var env = await ServiceSlot.GetEnvironmentAsync();
            if (_exitRequested) return;
            _ = Dispatcher.BeginInvoke(() =>
            {
                try
                {
                    var popup = new PopupWindow(slot.Def, env, uri, this);
                    popup.ApplyTheme(_theme.Current == AppTheme.Dark);
                    popup.Closed += (_, _) =>
                    {
                        // OAuth callbacks set cookies in the shared profile; reload the
                        // originating service so the signed-in state is reflected at once.
                        if (popup.IsAuthFlow) slot.Reload();
                    };
                    popup.Show();
                }
                catch (Exception ex)
                {
                    AppLog.Exception(ex, $"opening popup for {slot.Id}");
                }
            });
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"popup env for {slot.Id}");
        }
    }

    private void OnSlotRecreateRequested(ServiceSlot slot)
    {
        Dispatcher.BeginInvoke(async () =>
        {
            AppLog.Warn($"recreating webview for {slot.Id}");
            await slot.RecreateAsync();
            if (_activeServiceId == slot.Id)
            {
                ApplyMemoryPolicy();
            }
            UpdateOverlay();
        });
    }

    private void OnSlotWebNotification(ServiceSlot slot, CoreWebView2NotificationReceivedEventArgs e)
        => _router.OnWebNotification(slot.Def, e);

    private void OnSlotLiveActivityObserved(ServiceSlot slot, string title, string body, DateTimeOffset at, string? target, int? unread)
    {
        AppLog.Info($"live activity observed [{slot.Id}] {JsonSerializer.Serialize(title)}");
        Dispatcher.BeginInvoke(() =>
        {
            try
            {
                _activity.Add(slot.Id, slot.Def.DisplayName, slot.Def.Icon, title, body, at, target, unread);
                AppLog.Info($"live activity stored [{slot.Id}]");
                if (!_showingActivity && IsServiceOnScreen(slot.Id))
                    _activity.MarkRead(slot.Id, title);
                else if (IsActivityChat(slot.Id, title))
                    _activity.MarkRead(slot.Id, title);
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, $"persisting live activity [{slot.Id}]");
            }
        });
    }

    // ── Native WhatsApp ──────────────────────────────────────────────

    private void OnNativeWhatsAppMessage(NativeWhatsAppMessage message)
    {
        Dispatcher.BeginInvoke(() =>
        {
            try
            {
                var item = _items.FirstOrDefault(i =>
                    string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase));
                if (item == null) return;

                // While the WebView is alive it owns WhatsApp observation. The native
                // helper remains connected so the switch back to hibernation is instant.
                if (item.Slot.IsLive) return;

                var at = message.At == default ? DateTimeOffset.Now : message.At;
                _activity.Add("whatsapp", "WhatsApp", "whatsapp", message.Title, message.Body, at);
                AppLog.Info($"native WhatsApp message stored [{message.Title}]");
                // The WebView is asleep, so "on screen" here is only the overlay.
                // Mark read only when this conversation is actually selected in the inbox.
                if (IsActivityChat("whatsapp", message.Title))
                    _activity.MarkRead("whatsapp", message.Title);

                UpdateNativeWhatsAppBadge();
                if (IsActivityChat("whatsapp", message.Title)) return;
                if (ResolveNotificationMode("whatsapp") == NotificationMode.Off) return;
                if (!_nativeWhatsAppGate.ShouldNotify(
                        NotificationGate.KeyFor(message.Title, message.Body), DateTimeOffset.UtcNow))
                    return;

                _toasts.Show("whatsapp", message.Title, message.Body, _settings.NotificationSound);
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, "native WhatsApp activity");
            }
        });
    }

    private void OnNativeWhatsAppStatusChanged()
    {
        Dispatcher.BeginInvoke(() =>
        {
            try
            {
                var item = _items.FirstOrDefault(i =>
                    string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase));
                var status = _nativeWhatsApp.Status;
                AppLog.Info($"native WhatsApp status: {status.State} paired={status.Paired}");

                if (item != null && status is { State: "connected", Paired: true } &&
                    item.Slot.IsLive && !IsServiceOnScreen("whatsapp") && !_showingActivity)
                {
                    // Pairing just completed (or reconnected) while WhatsApp was not on
                    // screen. Put the heavy WebView back to sleep immediately.
                    item.Slot.Hibernate();
                    item.IsDormant = true;
                    AppLog.Info("WhatsApp WebView hibernated after native pairing");
                }

                var nativeUnavailable = status.State == "logged_out" ||
                    (status.Paired && status.State is "error" or "stopped");
                if (item is { IsDormant: true } && nativeUnavailable)
                {
                    // The native helper is no longer able to own WhatsApp. Never leave
                    // the user with a dormant WebView and no message source.
                    item.IsDormant = false;
                    _ = item.Slot.CreateAsync();
                    _settings.HibernatedServices.RemoveAll(id =>
                        string.Equals(id, "whatsapp", StringComparison.OrdinalIgnoreCase));
                    _store.Save(_settings);
                    AppLog.Warn($"native WhatsApp unavailable ({status.State}); waking WebView");
                }

                UpdateNativeWhatsAppBadge();
                UpdateActivitySleepBanner();
                UpdateOverlay();
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, "native WhatsApp status change");
            }
        });
    }

    private void UpdateNativeWhatsAppBadge()
    {
        var item = _items.FirstOrDefault(i =>
            string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase));
        if (item == null || item.Slot.IsLive) return;
        if (!_nativeWhatsApp.IsEnabled || !_nativeWhatsApp.IsPaired) return;
        _badges.SetExact("whatsapp", _activity.UnreadFor("whatsapp"));
    }

    /// <summary>Settings: switch native WhatsApp collection on/off without a restart.</summary>
    public void SetNativeWhatsAppEnabled(bool enabled)
    {
        _settings.NativeWhatsAppEnabled = enabled;
        if (!enabled)
        {
            _nativeWhatsApp.Stop();
            var item = _items.FirstOrDefault(i =>
                string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase));
            if (item is { IsDormant: true })
            {
                item.IsDormant = false;
                _ = item.Slot.CreateAsync();
            }
        }
        else
        {
            _nativeWhatsApp.Start();
        }

        _store.Save(_settings);
        UpdateNativeWhatsAppBadge();
        UpdateActivitySleepBanner();
        UpdateOverlay();
    }

    // ── Unified inbox ────────────────────────────────────────────────

    private const int ActivityDisplayLimit = 150;

    private void OnNotificationObserved(ServiceDefinition definition, string title, string body, DateTimeOffset at)
    {
        // NotificationReceived is synchronous with the site's Notification() call.
        // Never serialize activity.json on WebView2's callback thread.
        Dispatcher.BeginInvoke(() =>
        {
            try
            {
                _activity.Add(definition.Id, definition.DisplayName, definition.Icon, title, body, at);
                AppLog.Info($"activity stored [{definition.Id}]");
                // Only the conversation actually open in the pane (or a full-screen
                // service) is auto-read; other chats must keep their unread badge.
                if (!_showingActivity && IsServiceOnScreen(definition.Id))
                    _activity.MarkRead(definition.Id, title);
                else if (IsActivityChat(definition.Id, title))
                    _activity.MarkRead(definition.Id, title);
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, $"persisting activity [{definition.Id}]");
            }
        });
    }

    private void RefreshActivity()
    {
        var enabled = new HashSet<string>(_items.Select(i => i.Id), StringComparer.OrdinalIgnoreCase);
        var entries = _activity.Snapshot().Where(e => enabled.Contains(e.ServiceId)).ToList();
        var unread = entries.Sum(e => Math.Max(0, e.Count));

        var ordered = new List<ActivityRow>(Math.Min(entries.Count, ActivityDisplayLimit));
        foreach (var entry in entries.Take(ActivityDisplayLimit))
        {
            var key = ActivityRow.KeyFor(entry);
            if (_activityCache.TryGetValue(key, out var row))
            {
                row.Update(entry);
            }
            else
            {
                row = new ActivityRow(entry);
                _activityCache[key] = row;
            }
            ordered.Add(row);
        }

        var live = new HashSet<string>(ordered.Select(r => r.Key), StringComparer.OrdinalIgnoreCase);
        foreach (var stale in _activityCache.Keys.Where(k => !live.Contains(k)).ToList())
            _activityCache.Remove(stale);

        // Update the observable rows in place. Clearing and re-adding the whole
        // ItemsSource on every live event made WPF drop the selection, which could
        // open an unrelated conversation moments after the user picked one.
        _rebuildingActivity = true;
        try
        {
            for (var i = _activityRows.Count - 1; i >= 0; i--)
            {
                if (!live.Contains(_activityRows[i].Key))
                    _activityRows.RemoveAt(i);
            }

            for (var i = 0; i < ordered.Count; i++)
            {
                var row = ordered[i];
                var at = _activityRows.IndexOf(row);
                if (at < 0) _activityRows.Insert(i, row);
                else if (at != i) _activityRows.Move(at, i);
            }

            if (_activitySelectedKey != null)
            {
                var selected = _activityRows.FirstOrDefault(r =>
                    string.Equals(r.Key, _activitySelectedKey, StringComparison.OrdinalIgnoreCase));
                if (!ReferenceEquals(ActivityList.SelectedItem, selected))
                    ActivityList.SelectedItem = selected;
            }
        }
        finally
        {
            _rebuildingActivity = false;
        }

        ActivityList.Visibility = ordered.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
        ActivityEmpty.Visibility = ordered.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        ActivityMarkAllButton.IsEnabled = unread > 0;

        ActivitySubtitle.Text = entries.Count == 0
            ? "New messages from every connected service, with the service logo."
            : unread == 0
                ? $"{entries.Count} conversation{(entries.Count == 1 ? "" : "s")} · all read"
                : $"{entries.Count} conversation{(entries.Count == 1 ? "" : "s")} · {unread} unread";

        AllBadge.Visibility = unread > 0 ? Visibility.Visible : Visibility.Collapsed;
        AllBadgeText.Text = unread > 99 ? "99+" : unread.ToString();

        UpdateNativeWhatsAppBadge();
        UpdateActivitySleepBanner();
    }

    private void UpdateActivitySleepBanner()
    {
        var nativeWhatsApp = _nativeWhatsApp.IsEnabled && _nativeWhatsApp.IsPaired;
        var dormant = _items
            .Where(i => i.IsDormant && !(nativeWhatsApp &&
                string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase)))
            .Select(i => i.Def.DisplayName)
            .ToList();

        if (dormant.Count == 0 && !(nativeWhatsApp && _items.Any(i =>
                string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase) && i.IsDormant)))
        {
            ActivitySleepBanner.Visibility = Visibility.Collapsed;
            return;
        }

        ActivitySleepBanner.Visibility = Visibility.Visible;
        if (dormant.Count == 0)
        {
            ActivitySleepText.Text =
                "WhatsApp is collected natively while its WebView sleeps; open WhatsApp here to read and reply.";
            return;
        }

        ActivitySleepText.Text = dormant.Count == 1
            ? $"{dormant[0]} is hibernated — its messages are not collected."
            : $"{dormant.Count} services are hibernated ({string.Join(", ", dormant.Take(3))}" +
              (dormant.Count > 3 ? ", …" : "") + ") — their messages are not collected.";
    }

    private void All_Checked(object sender, RoutedEventArgs e)
    {
        if (_suppressAllChecked || !_initialized) return;
        ShowActivity();
    }

        /// <summary>Shows the combined inbox. Only the selected chat's webview stays visible.</summary>
    public void ShowActivity()
    {
        if (!_initialized) return;
        _showingActivity = true;
        _activeServiceId = _activityChatServiceId;
        foreach (var item in _items) item.IsActive = false;

        ActiveServiceText.Text = "· All messages";
        StateOverlay.Visibility = Visibility.Collapsed;
        EmptyState.Visibility = Visibility.Collapsed;
        ActivityPanel.Visibility = Visibility.Visible;
        SetProgress(false);
        RefreshActivity();
        UpdateLayout();
        ApplyWebVisibility();
        ApplyMemoryPolicy();
    }

    private void ActivityList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_rebuildingActivity || _suppressActivitySelection) return;
        if (ActivityList.SelectedItem is not ActivityRow row) return;
        // A live refresh can re-select the row that is already open; do not re-run
        // the reveal (and never steal the pane from a conversation the user picked).
        if (_showingActivity &&
            string.Equals(row.Key, _activitySelectedKey, StringComparison.OrdinalIgnoreCase))
            return;

        _suppressActivitySelection = true;
        try
        {
            _activitySelectedKey = row.Key;
            _activity.MarkRead(row.Entry.ServiceId, row.Entry.Title);
            _ = OpenActivityChatAsync(row.Entry.ServiceId, row.Entry.Title, row.Entry.Target);
        }
        finally
        {
            _suppressActivitySelection = false;
        }
    }

    /// <summary>
    /// Opens the conversation's service in the right pane of the unified inbox.
    /// The service's own WebView2 is displayed there, so reading and replying use
    /// the native service editor; the host only brings the matching chat into view
    /// when the service supports it (best effort, never required for reply).
    /// </summary>
    private async Task OpenActivityChatAsync(string serviceId, string title, string? target = null)
    {
        var item = _items.FirstOrDefault(i =>
            string.Equals(i.Id, serviceId, StringComparison.OrdinalIgnoreCase));
        if (item == null) return;

        var token = ++_activityOpenToken;
        _showingActivity = true;
        _activityChatServiceId = serviceId;
        _activeServiceId = serviceId;
        ActiveServiceText.Text = "· All messages";

        if (item.IsDormant)
        {
            item.IsDormant = false;
            _settings.HibernatedServices.RemoveAll(id =>
                string.Equals(id, serviceId, StringComparison.OrdinalIgnoreCase));
            _store.Save(_settings);
            AppLog.Info($"service resumed for inbox chat: {serviceId}");
            RefreshActivity();
        }

        ActivityChatPlaceholder.Visibility = Visibility.Collapsed;
        UpdateLayout();
        ApplyWebVisibility();

        await item.Slot.CreateAsync();
        if (_exitRequested || token != _activityOpenToken) return;

        ApplyWebVisibility();
        ApplyMemoryPolicy();
        // The stored href is exact; title matching stays as the fallback for services
        // without stable row links (Telegram) or when the row is not rendered yet.
        if (string.IsNullOrWhiteSpace(target) || !await item.Slot.TryRevealTargetAsync(target))
            await item.Slot.TryRevealChatAsync(title);
    }

    private void ActivityMarkAll_Click(object sender, RoutedEventArgs e) => _activity.MarkAllRead();

    private void ActivityResumeAll_Click(object sender, RoutedEventArgs e) => ResumeAllServices();

    /// <summary>Wakes every hibernated service and clears the persisted sleep list in one save.</summary>
    public void ResumeAllServices()
    {
        var resumed = false;
        foreach (var item in _items.Where(i => i.IsDormant))
        {
            item.IsDormant = false;
            _ = item.Slot.CreateAsync();
            resumed = true;
        }

        if (!resumed) return;
        _settings.HibernatedServices.Clear();
        _store.Save(_settings);
        UpdateOverlay();
        RefreshActivity();
        AppLog.Info("all services resumed from the unified inbox");
    }

    private NotificationMode ResolveNotificationMode(string serviceId)
    {
        var item = _items.FirstOrDefault(i => i.Id == serviceId);
        if (item == null) return NotificationMode.Host;
        if (_settings.NotificationModes.TryGetValue(serviceId, out var raw) &&
            Enum.TryParse<NotificationMode>(raw, ignoreCase: true, out var parsed))
            return parsed;
        return item.Def.NotificationMode;
    }

    private bool IsServiceOnScreen(string serviceId)
        => !_showingActivity &&
           string.Equals(serviceId, _activeServiceId, StringComparison.OrdinalIgnoreCase) &&
           IsActive && IsVisible && WindowState != WindowState.Minimized;

    /// <summary>True when the notification belongs to the conversation open in the inbox pane.</summary>
    private bool IsActivityChat(string serviceId, string title)
    {
        if (!_showingActivity || string.IsNullOrEmpty(_activitySelectedKey)) return false;
        var selected = _activityRows.FirstOrDefault(r =>
            string.Equals(r.Key, _activitySelectedKey, StringComparison.OrdinalIgnoreCase));
        return selected != null &&
               string.Equals(selected.Entry.ServiceId, serviceId, StringComparison.OrdinalIgnoreCase) &&
               string.Equals(selected.Entry.Title, title, StringComparison.OrdinalIgnoreCase);
    }

    // ── Badges / tray ────────────────────────────────────────────────

    private void UpdateBadges()
    {
        foreach (var item in _items) item.Unread = _badges.Get(item.Id);
        var total = _badges.Total;
        TotalBadge.Visibility = total > 0 ? Visibility.Visible : Visibility.Collapsed;
        TotalBadgeText.Text = total > 99 ? "99+" : total.ToString();
        UnreadTotalChanged?.Invoke(total);
    }

    // ── Memory policy ────────────────────────────────────────────────

    public void ApplyMemoryPolicy()
    {
        if (!_initialized || _exitRequested) return;
        var policy = new MemoryGovernorPolicy(_settings.AutoLowMemory);
        var windowVisible = IsActive;
        foreach (var item in _items)
            item.Slot.ApplyMemory(policy.Decide(item.Slot.State, item.Id, _activeServiceId ?? "", windowVisible, userHibernated: false));
    }

    // ── Overlay ──────────────────────────────────────────────────────

    private void UpdateOverlay()
    {
        if (_showingActivity)
        {
            StateOverlay.Visibility = Visibility.Collapsed;
            EmptyState.Visibility = Visibility.Collapsed;
            ApplyWebVisibility();
            return;
        }

        var item = ActiveItem;
        if (item == null)
        {
            StateOverlay.Visibility = Visibility.Collapsed;
            EmptyState.Visibility = _items.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            ApplyWebVisibility();
            return;
        }

        EmptyState.Visibility = Visibility.Collapsed;
        var slot = item.Slot;

        if (slot.State == ServiceState.Starting)
        {
            SetOverlayIcon(item);
            OverlayTitle.Text = $"Loading {item.Def.DisplayName}…";
            OverlayMessage.Text = item.IsDormant ? "Resuming from hibernation" : "";
            OverlayPrimary.Visibility = Visibility.Collapsed;
            OverlaySecondary.Visibility = Visibility.Collapsed;
            StateOverlay.Visibility = Visibility.Visible;
        }
        else if (slot.State == ServiceState.Failed)
        {
            SetOverlayIcon(item);
            OverlayTitle.Text = $"Couldn't load {item.Def.DisplayName}";
            OverlayMessage.Text = slot.LastError ?? "Unknown error";
            OverlayPrimary.Content = "Try again";
            OverlayPrimary.Visibility = Visibility.Visible;
            OverlaySecondary.Visibility = Visibility.Visible;
            StateOverlay.Visibility = Visibility.Visible;
        }
        else if (slot.State is ServiceState.Hibernated or ServiceState.NotCreated)
        {
            SetOverlayIcon(item);
            var nativeWhatsApp = string.Equals(item.Id, "whatsapp", StringComparison.OrdinalIgnoreCase) &&
                                 _nativeWhatsApp.IsEnabled && _nativeWhatsApp.IsPaired;
            OverlayTitle.Text = nativeWhatsApp
                ? "WhatsApp is running natively"
                : $"{item.Def.DisplayName} is hibernated";
            OverlayMessage.Text = nativeWhatsApp
                ? "New messages appear in All messages. Open WhatsApp here to read and reply."
                : "The session is kept on disk. Resume or click the service to wake it up.";
            OverlayPrimary.Content = nativeWhatsApp ? "Open WhatsApp" : "Resume";
            OverlayPrimary.Visibility = Visibility.Visible;
            OverlaySecondary.Visibility = Visibility.Collapsed;
            StateOverlay.Visibility = Visibility.Visible;
        }
        else
        {
            StateOverlay.Visibility = Visibility.Collapsed;
        }

        ApplyWebVisibility();
    }

    /// <summary>
    /// Airspace: a hosted WebView2 (HWND) always paints above WPF content, so exactly one
    /// slot is visible and it is hidden while a loading/failure overlay is shown.
    /// </summary>
    private void ApplyWebVisibility()
    {
        var overlayUp = StateOverlay.Visibility == Visibility.Visible ||
                        EmptyState.Visibility == Visibility.Visible;

        var leftInset = 0.0;
        if (_showingActivity && !string.IsNullOrEmpty(_activityChatServiceId))
        {
            leftInset = ActivityListColumn.ActualWidth;
            if (leftInset <= 0) leftInset = ActivityListColumn.Width.Value;
            leftInset += 1; // left pane border
        }

        foreach (var item in _items)
        {
            var inboxChat = _showingActivity &&
                            string.Equals(item.Id, _activityChatServiceId, StringComparison.OrdinalIgnoreCase);
            var visible = !overlayUp && (_showingActivity
                ? inboxChat
                : string.Equals(item.Id, _activeServiceId, StringComparison.OrdinalIgnoreCase));

            item.Slot.SetInboxFocusMode(inboxChat);
            item.Slot.SetPaneInsets(leftInset, 0, 0, 0);
            item.Slot.Show(visible);
        }

        var chatVisible = _showingActivity && !overlayUp && _items.Any(i =>
            string.Equals(i.Id, _activityChatServiceId, StringComparison.OrdinalIgnoreCase) && i.Slot.IsLive);
        ActivityChatPlaceholder.Visibility = chatVisible ? Visibility.Collapsed : Visibility.Visible;
    }

    /// <summary>Overlay identity icon: colored brand mark when available, monochrome glyph otherwise.</summary>
    private void SetOverlayIcon(SidebarItem item)
    {
        if (item.IconImage != null)
        {
            OverlayImage.Source = item.IconImage;
            OverlayImage.Visibility = Visibility.Visible;
            OverlayIcon.Visibility = Visibility.Collapsed;
        }
        else
        {
            OverlayIcon.Data = item.Icon;
            OverlayIcon.Visibility = Visibility.Visible;
            OverlayImage.Visibility = Visibility.Collapsed;
        }
    }

    private void OverlayPrimary_Click(object sender, RoutedEventArgs e)
    {
        var item = ActiveItem;
        if (item == null) return;

        // The same button is "Resume" for a hibernated slot and "Try again" for a failure.
        if (item.IsDormant || item.Slot.State is ServiceState.Hibernated or ServiceState.NotCreated)
            ToggleHibernate(item.Id);
        else
            item.Slot.Reload();
    }

    private void OverlaySecondary_Click(object sender, RoutedEventArgs e)
    {
        var item = ActiveItem;
        if (item != null) ServiceSlot.OpenInSystemBrowser(item.Def.Url);
    }

    // ── Sidebar / chrome events ──────────────────────────────────────

    private void Service_Checked(object sender, RoutedEventArgs e)
    {
        if (sender is RadioButton { DataContext: SidebarItem item } && !item.IsActive)
            ActivateServiceCore(item.Id);
    }

    private void Ctx_Reload(object sender, RoutedEventArgs e)
        => MenuItemItem(sender)?.Slot.Reload();

    private void Ctx_Hibernate(object sender, RoutedEventArgs e)
    {
        if (MenuItemItem(sender) is { } item) ToggleHibernate(item.Id);
    }

    private void Ctx_OpenBrowser(object sender, RoutedEventArgs e)
    {
        if (MenuItemItem(sender) is { } item)
            ServiceSlot.OpenInSystemBrowser(item.Def.Url);
    }

    private void Ctx_ChangeLogo(object sender, RoutedEventArgs e)
    {
        if (MenuItemItem(sender) is { } item)
            PromptChangeServiceLogo(item.Id, this);
    }

    private void Ctx_ResetLogo(object sender, RoutedEventArgs e)
    {
        if (MenuItemItem(sender) is { } item)
            ResetServiceLogo(item.Id);
    }

    private void Ctx_Opened(object sender, RoutedEventArgs e)
    {
        if (sender is not ContextMenu menu) return;
        var reset = menu.Items.OfType<MenuItem>()
            .FirstOrDefault(m => string.Equals(m.Header as string, "Reset logo", StringComparison.Ordinal));
        if (reset == null) return;

        var item = reset.DataContext as SidebarItem;
        if (item == null && reset.Tag is string id)
            item = _items.FirstOrDefault(i => string.Equals(i.Id, id, StringComparison.OrdinalIgnoreCase));
        reset.IsEnabled = item != null && IconStore.FindOverride(item.Id) != null;
    }

    /// <summary>ContextMenus live outside the visual tree; resolve the owning item via the
    /// bound DataContext first, then the Tag id, so every menu action targets its own service.</summary>
    private SidebarItem? MenuItemItem(object sender)
    {
        if (sender is not MenuItem mi) return null;
        if (mi.DataContext is SidebarItem direct) return direct;
        if (mi.Tag is string id)
            return _items.FirstOrDefault(i => string.Equals(i.Id, id, StringComparison.OrdinalIgnoreCase));
        return null;
    }

    private void Settings_Click(object sender, RoutedEventArgs e) => OpenSettings();

    public void OpenSettings()
    {
        if (_settingsWindow == null || !_settingsWindow.IsLoaded)
        {
            _settingsWindow = new SettingsWindow(_settings, _store, _theme, _metrics, this, _toasts);
            _settingsWindow.Closed += (_, _) => _settingsWindow = null;
            _settingsWindow.Show();
        }

        if (_settingsWindow.WindowState == WindowState.Minimized)
            _settingsWindow.WindowState = WindowState.Normal;

        // Bring the owned window in front even when the main HWND currently hosts
        // a focused WebView2 surface.
        _settingsWindow.Activate();
        _settingsWindow.Topmost = true;
        _settingsWindow.Topmost = false;
    }

    private void Min_Click(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;

    private void Max_Click(object sender, RoutedEventArgs e)
        => WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    private void OnPreviewKeyDown(object sender, KeyEventArgs e)
    {
        if (Keyboard.Modifiers != ModifierKeys.Control) return;

        var index = e.Key switch
        {
            >= Key.D1 and <= Key.D9 => e.Key - Key.D1,
            >= Key.NumPad1 and <= Key.NumPad9 => e.Key - Key.NumPad1,
            _ => -1,
        };
        if (index >= 0 && index < _items.Count)
        {
            ActivateServiceCore(_items[index].Id);
            e.Handled = true;
            return;
        }

        switch (e.Key)
        {
            case Key.OemComma:
                OpenSettings();
                e.Handled = true;
                break;
            case Key.D0:
            case Key.NumPad0:
                ShowActivity();
                e.Handled = true;
                break;
            case Key.H when !_showingActivity && ActiveItem is { } item:
                ToggleHibernate(item.Id);
                e.Handled = true;
                break;
            case Key.M:
                WindowState = WindowState.Minimized;
                e.Handled = true;
                break;
        }
    }

    // ── Theme / DWM ──────────────────────────────────────────────────

    private void OnThemeChanged(AppTheme theme) => Dispatcher.BeginInvoke(() =>
    {
        ApplyDwmTheme();
        var dark = theme == AppTheme.Dark;
        foreach (var item in _items) item.Slot.ApplyTheme(dark);
        foreach (Window window in Application.Current.Windows)
            if (window is PopupWindow popup) popup.ApplyTheme(dark);
    });

    private void ApplyDwmTheme()
    {
        try
        {
            var hwnd = new WindowInteropHelper(this).Handle;
            if (hwnd == IntPtr.Zero) return;
            var dark = _theme.Current == AppTheme.Dark;
            var value = dark ? 1 : 0;
            // Windows 10 20H1+ uses 20; older builds used 19.
            if (NativeMethods.DwmSetWindowAttribute(hwnd, 20, ref value, sizeof(int)) != 0)
                NativeMethods.DwmSetWindowAttribute(hwnd, 19, ref value, sizeof(int));
        }
        catch
        {
            // cosmetic only
        }
    }

    protected override void OnSourceInitialized(EventArgs e)
    {
        base.OnSourceInitialized(e);
        var source = HwndSource.FromHwnd(new WindowInteropHelper(this).Handle);
        source?.AddHook(WndProc);
        ApplyDwmTheme();
    }

    private IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        const int WM_GETMINMAXINFO = 0x0024;
        const int WM_SETTINGCHANGE = 0x001A;

        switch (msg)
        {
            case WM_GETMINMAXINFO:
                // WindowStyle=None windows maximize over the taskbar without this.
                HandleGetMinMaxInfo(hwnd, lParam);
                handled = true;
                break;

            case WM_SETTINGCHANGE:
                var text = Marshal.PtrToStringUni(lParam);
                if (string.Equals(text, "ImmersiveColorSet", StringComparison.OrdinalIgnoreCase))
                    _watcher.Notify(); // theme switched (live wallpaper/theme changes)
                break;
        }
        return IntPtr.Zero;

        static void HandleGetMinMaxInfo(IntPtr hwnd, IntPtr lParam)
        {
            var mmi = Marshal.PtrToStructure<MINMAXINFO>(lParam);
            var monitor = NativeMethods.MonitorFromWindow(hwnd, NativeMethods.MONITOR_DEFAULTTONEAREST);
            if (monitor != IntPtr.Zero)
            {
                var mi = new NativeMethods.MONITORINFO { cbSize = Marshal.SizeOf<NativeMethods.MONITORINFO>() };
                if (NativeMethods.GetMonitorInfo(monitor, ref mi))
                {
                    mmi.ptMaxPosition.x = mi.rcWork.left - mi.rcMonitor.left;
                    mmi.ptMaxPosition.y = mi.rcWork.top - mi.rcMonitor.top;
                    mmi.ptMaxSize.x = mi.rcWork.right - mi.rcWork.left;
                    mmi.ptMaxSize.y = mi.rcWork.bottom - mi.rcWork.top;
                }
            }
            Marshal.StructureToPtr(mmi, lParam, true);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINTAPI { public int x, y; }

    [StructLayout(LayoutKind.Sequential)]
    private struct MINMAXINFO
    {
        public POINTAPI ptReserved, ptMaxSize, ptMaxPosition, ptMinTrackSize, ptMaxTrackSize;
    }

    // ── Closing / persistence ────────────────────────────────────────

    protected override void OnClosing(CancelEventArgs e)
    {
        base.OnClosing(e);
        if (_exitRequested) return;

        e.Cancel = true;
        if (_settings.CloseMinimizesToTray) Hide();
        else (Application.Current as App)?.RequestExit();
    }

    public void PrepareExit()
    {
        _exitRequested = true;
        PersistSettings();
        try { _nativeWhatsApp.Dispose(); } catch { /* best effort */ }
        try
        {
            foreach (var slot in _slots) slot.Dispose();
        }
        catch { /* best effort */ }
        try { Close(); } catch { /* already closing */ }
    }

    private void PersistSettings()
    {
        try
        {
            var bounds = WindowState == WindowState.Normal ? new Rect(Left, Top, Width, Height) : RestoreBounds;
            if (!double.IsNaN(bounds.Left) && bounds.Width >= 100 && bounds.Height >= 100)
            {
                _settings.WindowLeft = bounds.Left;
                _settings.WindowTop = bounds.Top;
                _settings.WindowWidth = bounds.Width;
                _settings.WindowHeight = bounds.Height;
            }
            // Native-mode WhatsApp is intentionally asleep, not manually hibernated.
            var nativeWhatsApp = _nativeWhatsApp.IsEnabled && _nativeWhatsApp.IsPaired;
            _settings.HibernatedServices = _items
                .Where(i => i.IsDormant && !(nativeWhatsApp &&
                    string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase)))
                .Select(i => i.Id)
                .ToList();
            _store.Save(_settings);
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, "persisting settings");
        }
    }

    private void RestoreWindowGeometry()
    {
        var area = SystemParameters.WorkArea;
        var hasSaved = !double.IsNaN(_settings.WindowWidth) && _settings.WindowWidth >= 100 &&
                       !double.IsNaN(_settings.WindowHeight) && _settings.WindowHeight >= 100;

        if (!hasSaved)
        {
            // First run: keep the default size on-screen even on a short display.
            Width = Math.Min(Width, Math.Max(MinWidth, area.Width));
            Height = Math.Min(Height, Math.Max(MinHeight, area.Height));
            return;
        }

        // Saved size may come from a larger monitor; never restore taller/wider than
        // the current work area, otherwise the caption and sidebar footer sit off-screen.
        Width = Math.Min(Math.Max(MinWidth, _settings.WindowWidth), Math.Max(MinWidth, area.Width));
        Height = Math.Min(Math.Max(MinHeight, _settings.WindowHeight), Math.Max(MinHeight, area.Height));
        Left = _settings.WindowLeft;
        Top = _settings.WindowTop;

        // Keep the window on a visible monitor.
        if (double.IsNaN(Left) || double.IsNaN(Top) ||
            Left + 80 > area.Right || Left + Width < area.Left + 80 ||
            Top + 40 > area.Bottom || Top + Height < area.Top + 40)
        {
            Left = area.Left + Math.Max(0, (area.Width - Width) / 2);
            Top = area.Top + Math.Max(0, (area.Height - Height) / 2);
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────

    public static Geometry LoadIcon(string key)
    {
        try
        {
            var resourceKey = "Icon." + char.ToUpperInvariant(key[0]) + key[1..];
            if (Application.Current.TryFindResource(resourceKey) is Geometry g) return g;
        }
        catch
        {
            // fall through to globe
        }
        return (Geometry)Application.Current.FindResource("Icon.Globe");
    }
}
