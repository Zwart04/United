using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;
using Microsoft.Web.WebView2.Core;
using United.App.Diagnostics;
using United.App.Services;
using United.App.Shell;
using United.Core.Lifecycle;
using United.Core.Services;
using United.Core.Settings;
using United.Platform.Windows.Activation;
using United.Platform.Windows.Metrics;
using United.Platform.Windows.Startup;

namespace United.App.Views;

public partial class SettingsWindow : Window
{
    private readonly AppSettings _settings;
    private readonly SettingsStore _store;
    private readonly ThemeManager _theme;
    private readonly ProcessMetricsCollector _metrics;
    private readonly MainWindow _host;
    private readonly Platform.Windows.Notifications.ToastService _toasts;

    private readonly DispatcherTimer _perfTimer;
    private readonly Dictionary<string, TextBlock> _stateLabels = new(StringComparer.OrdinalIgnoreCase);
    private bool _loading = true;
    private bool _updateBusy;
    private bool _updateChecked;

    public SettingsWindow(AppSettings settings, SettingsStore store, ThemeManager theme,
        ProcessMetricsCollector metrics, MainWindow host, Platform.Windows.Notifications.ToastService toasts)
    {
        _settings = settings;
        _store = store;
        _theme = theme;
        _metrics = metrics;
        _host = host;
        _toasts = toasts;

        // Owned window: stays above United, centres on it, and is never hidden
        // behind the main HWND when a WebView2 service has focus.
        Owner = host;

        InitializeComponent();

        foreach (var nav in new[] { NavGeneral, NavServices, NavPerformance, NavAbout })
            nav.PreviewMouseLeftButtonDown += Nav_PreviewMouseLeftButtonDown;

        LoadGeneral();
        BuildServiceRows();
        LoadAbout();

        _perfTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(1500) };
        _perfTimer.Tick += (_, _) => RefreshPerformance();
        _perfTimer.Start();
        RefreshPerformance();

        _loading = false;
        Closed += (_, _) => _perfTimer.Stop();
    }

    // ── Navigation ─────────────────────────────────────────────────

    // Select on mouse-down. On a cold start the window may still be activating,
    // and the button-up can be lost, which made the first tab click do nothing.
    private void Nav_PreviewMouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (sender is RadioButton nav)
        {
            nav.IsChecked = true;
            e.Handled = true;
        }
    }

    private void Nav_Checked(object sender, RoutedEventArgs e)
    {
        // Checked fires while InitializeComponent loads NavGeneral before the panels exist.
        if (PanelGeneral is null) return;

        PanelGeneral.Visibility = NavGeneral.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        PanelServices.Visibility = NavServices.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        PanelPerformance.Visibility = NavPerformance.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;
        PanelAbout.Visibility = NavAbout.IsChecked == true ? Visibility.Visible : Visibility.Collapsed;

        if (NavServices.IsChecked == true) BuildServiceRows();
        if (NavPerformance.IsChecked == true) RefreshPerformance();
        if (NavAbout.IsChecked == true)
        {
            RefreshUpdateStatus();
            if (_settings.CheckForUpdatesOnStartup && !_updateBusy && !_updateChecked)
                _ = CheckForUpdatesAsync(force: false);
        }
    }

    // ── General ────────────────────────────────────────────────────

    private void LoadGeneral()
    {
        _loading = true;
        ThemeCombo.Items.Clear();
        foreach (var label in new[] { "System", "Light", "Dark" }) ThemeCombo.Items.Add(label);
        ThemeCombo.SelectedIndex = _settings.Theme?.Trim().ToLowerInvariant() switch
        {
            "light" => 1,
            "dark" => 2,
            _ => 0,
        };

        LaunchAtStartupCheck.IsChecked = _settings.LaunchAtStartup;
        StartMinimizedCheck.IsChecked = _settings.StartMinimized;
        CloseToTrayCheck.IsChecked = _settings.CloseMinimizesToTray;
        TrayBadgeCheck.IsChecked = _settings.ShowTrayBadge;
        SoundCheck.IsChecked = _settings.NotificationSound;
        AutoLowMemoryCheck.IsChecked = _settings.AutoLowMemory;
        NativeWhatsAppCheck.IsChecked = _settings.NativeWhatsAppEnabled;
        RefreshNativeWhatsAppStatus();
        _loading = false;
    }

    private void TestNotification_Click(object sender, RoutedEventArgs e)
    {
        var target = _host.Items.FirstOrDefault()?.Id ?? "whatsapp";
        _toasts.Show(target, "Test notification",
            "United is delivering Windows toasts through the same path as service notifications.",
            _settings.NotificationSound);
    }

    private void ThemeCombo_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;
        _settings.Theme = ThemeCombo.SelectedIndex switch
        {
            1 => "light",
            2 => "dark",
            _ => "system",
        };
        _theme.Apply(_settings.Theme);
        _store.Save(_settings);
    }

    private void SettingChanged(object sender, RoutedEventArgs e)
    {
        if (_loading) return;

        _settings.LaunchAtStartup = LaunchAtStartupCheck.IsChecked == true;
        _settings.StartMinimized = StartMinimizedCheck.IsChecked == true;
        _settings.CloseMinimizesToTray = CloseToTrayCheck.IsChecked == true;
        _settings.ShowTrayBadge = TrayBadgeCheck.IsChecked == true;
        _settings.NotificationSound = SoundCheck.IsChecked == true;
        _settings.AutoLowMemory = AutoLowMemoryCheck.IsChecked == true;

        var nativeEnabled = NativeWhatsAppCheck.IsChecked == true;
        if (nativeEnabled != _settings.NativeWhatsAppEnabled)
            _host.SetNativeWhatsAppEnabled(nativeEnabled);
        else
            _settings.NativeWhatsAppEnabled = nativeEnabled;

        if (Environment.ProcessPath is { } exe)
            Autostart.SetEnabled(_settings.LaunchAtStartup, exe, _settings.StartMinimized);

        _host.ApplyMemoryPolicy();
        _store.Save(_settings);
        RefreshNativeWhatsAppStatus();
    }

    private void PairNativeWhatsApp_Click(object sender, RoutedEventArgs e)
    {
        if (!_host.NativeWhatsApp.IsAvailable)
        {
            MessageBox.Show(this,
                "United.WhatsAppBridge.exe is missing from this build.\n\n" +
                "Install the release package again, or rebuild the native bridge.",
                "United - native WhatsApp", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }

        if (!_settings.NativeWhatsAppEnabled)
        {
            NativeWhatsAppCheck.IsChecked = true; // triggers SetNativeWhatsAppEnabled
        }

        _host.NativeWhatsApp.Start();
        var window = new NativeWhatsAppPairingWindow(_host.NativeWhatsApp) { Owner = this };
        window.ShowDialog();
        RefreshNativeWhatsAppStatus();
        BuildServiceRows();
    }

    private void ForgetNativeWhatsApp_Click(object sender, RoutedEventArgs e)
    {
        if (!_host.NativeWhatsApp.IsAvailable) return;
        var answer = MessageBox.Show(this,
            "Remove the native WhatsApp link and its local session?\n\n" +
            "The normal WhatsApp WebView will be used again. You can pair it later.",
            "United - native WhatsApp", MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (answer != MessageBoxResult.Yes) return;

        _host.NativeWhatsApp.Forget();
        var item = _host.Items.FirstOrDefault(i =>
            string.Equals(i.Id, "whatsapp", StringComparison.OrdinalIgnoreCase));
        if (item is { IsDormant: true })
        {
            item.IsDormant = false;
            _ = item.Slot.CreateAsync();
        }
        RefreshNativeWhatsAppStatus();
        BuildServiceRows();
    }

    private void RefreshNativeWhatsAppStatus()
    {
        if (NativeWhatsAppStatus is null || NativeWhatsAppPairButton is null) return;

        var bridge = _host.NativeWhatsApp;
        NativeWhatsAppPairButton.IsEnabled = bridge.IsAvailable && _settings.NativeWhatsAppEnabled;
        if (!bridge.IsAvailable)
        {
            NativeWhatsAppStatus.Text =
                "Native bridge not found in this build. WhatsApp continues in the normal WebView.";
            return;
        }
        if (!_settings.NativeWhatsAppEnabled)
        {
            NativeWhatsAppStatus.Text = "Native collection is off; WhatsApp runs in its WebView.";
            return;
        }

        var status = bridge.Status;
        NativeWhatsAppStatus.Text = status.State switch
        {
            "connected" => "Connected natively. The WhatsApp WebView stays asleep until you open a chat.",
            "pairing" => "Waiting for the QR scan on your phone (WhatsApp > Linked devices).",
            "pairing_timeout" => "The QR expired. Open the pairing window again for a fresh code.",
            "disconnected" => "Paired, reconnecting to WhatsApp...",
            "logged_out" => "The phone unlinked this device. Pair again to resume native collection.",
            "error" => "Native bridge error: " + (status.Error ?? "unknown"),
            "stopped" => "Native bridge stopped. Reopen Settings or restart United to retry.",
            _ => status.Paired ? "Paired, waiting for connection..." : "Waiting for pairing.",
        };
    }

    // ── Services ───────────────────────────────────────────────────

    private void BuildServiceRows()
    {
        ServiceRows.Children.Clear();
        _stateLabels.Clear();

        foreach (var item in _host.Items)
        {
            var row = new Border
            {
                Background = (Brush)FindResource("BgCard"),
                BorderBrush = (Brush)FindResource("Border"),
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(8),
                Padding = new Thickness(12, 9, 12, 9),
                Margin = new Thickness(0, 0, 0, 6),
            };

            var grid = new Grid();
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto }); // icon
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto }); // enabled
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto }); // mode
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

            var icon = BuildIcon(item.Def, 26);
            icon.Margin = new Thickness(0, 1, 10, 0);
            icon.VerticalAlignment = VerticalAlignment.Top;
            Grid.SetColumn(icon, 0);
            grid.Children.Add(icon);

            var textCol = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
            textCol.Children.Add(new TextBlock
            {
                Text = item.Def.DisplayName,
                FontWeight = FontWeights.SemiBold,
            });
            textCol.Children.Add(new TextBlock
            {
                Text = item.Def.Url,
                FontSize = 11,
                Opacity = 0.65,
                TextTrimming = TextTrimming.CharacterEllipsis,
                Margin = new Thickness(0, 1, 0, 0),
            });
            var stateText = new TextBlock
            {
                Text = $"state {StateLabel(item.Slot.State)}  ·  id {item.Id}",
                FontSize = 11,
                Opacity = 0.65,
            };
            textCol.Children.Add(stateText);
            _stateLabels[item.Id] = stateText;
            Grid.SetColumn(textCol, 1);
            grid.Children.Add(textCol);

            // Enable checkbox
            var enable = new CheckBox
            {
                Content = "Enabled",
                IsChecked = true,
                VerticalAlignment = VerticalAlignment.Top,
                Margin = new Thickness(10, 0, 14, 0),
            };
            enable.Checked += (_, _) => SetEnabled(item.Def, true);
            enable.Unchecked += (_, _) => SetEnabled(item.Def, false);
            Grid.SetColumn(enable, 2);
            grid.Children.Add(enable);

            // Notification mode
            var mode = new ComboBox
            {
                Width = 110,
                MinWidth = 110,
                VerticalAlignment = VerticalAlignment.Top,
                Margin = new Thickness(0, -4, 0, 0),
            };
            foreach (var label in new[] { "Host", "Webview", "Off" }) mode.Items.Add(label);
            mode.SelectedIndex = item.Def.NotificationMode switch
            {
                NotificationMode.Webview => 1,
                NotificationMode.Off => 2,
                _ => 0,
            };
            if (_settings.NotificationModes.TryGetValue(item.Def.Id, out var raw) &&
                Enum.TryParse<NotificationMode>(raw, true, out var parsed))
                mode.SelectedIndex = (int)parsed;
            mode.SelectionChanged += (_, _) =>
            {
                _settings.NotificationModes[item.Def.Id] = ((NotificationMode)mode.SelectedIndex).ToString();
                _store.Save(_settings);
            };
            Grid.SetColumn(mode, 3);
            grid.Children.Add(mode);

            // Actions on their own line so long URLs and the logo keep room.
            var actions = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 8, 0, 0) };
            actions.Children.Add(ActionButton(IsHibernated(item) ? "Resume" : "Hibernate", () =>
            {
                _host.ToggleHibernate(item.Id);
                BuildServiceRows();
            }));
            actions.Children.Add(ActionButton("Reload", () =>
            {
                _host.Items.FirstOrDefault(i => i.Id == item.Id)?.Slot.Reload();
                BuildServiceRows();
            }));
            actions.Children.Add(ActionButton("Change logo...", () =>
            {
                if (_host.PromptChangeServiceLogo(item.Id, this)) BuildServiceRows();
            }));
            if (IconStore.FindOverride(item.Id) != null)
            {
                actions.Children.Add(ActionButton("Reset logo", () =>
                {
                    _host.ResetServiceLogo(item.Id);
                    BuildServiceRows();
                }));
            }
            if (ServiceCatalog.Find(item.Id) == null)
            {
                actions.Children.Add(ActionButton("Remove", () => RemoveCustomService(item)));
            }
            Grid.SetColumn(actions, 1);
            Grid.SetColumnSpan(actions, 3);
            Grid.SetRow(actions, 1);
            grid.Children.Add(actions);

            row.Child = grid;
            ServiceRows.Children.Add(row);
        }

        BuildAvailableRows();
        UpdateServicesHint();
    }

    private void BuildAvailableRows()
    {
        AvailableRows.Children.Clear();

        var enabled = new HashSet<string>(_host.Items.Select(i => i.Id), StringComparer.OrdinalIgnoreCase);
        var builtIns = ServiceCatalog.AllBuiltIn
            .Where(s => !enabled.Contains(s.Id))
            .ToList();
        var customs = CustomServicesFile.Load()
            .Where(s => !enabled.Contains(s.Id) && s.IsValid())
            .ToList();

        if (builtIns.Count == 0 && customs.Count == 0)
        {
            AvailableRows.Children.Add(new TextBlock
            {
                Text = "Every built-in service is already in your sidebar.",
                FontSize = 11,
                Opacity = 0.6,
                Margin = new Thickness(2, 2, 0, 0),
            });
            return;
        }

        foreach (var def in builtIns) AvailableRows.Children.Add(BuildAvailableRow(def, isCustom: false));
        foreach (var def in customs) AvailableRows.Children.Add(BuildAvailableRow(def, isCustom: true));
    }

    private Border BuildAvailableRow(ServiceDefinition def, bool isCustom)
    {
        var row = new Border
        {
            Background = (Brush)FindResource("BgCard"),
            BorderBrush = (Brush)FindResource("Border"),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(10, 6, 10, 6),
            Margin = new Thickness(0, 0, 0, 4),
        };

        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var icon = BuildIcon(def, 22);
        icon.Margin = new Thickness(0, 0, 10, 0);
        Grid.SetColumn(icon, 0);
        grid.Children.Add(icon);

        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        text.Children.Add(new TextBlock { Text = def.DisplayName, FontWeight = FontWeights.SemiBold });
        text.Children.Add(new TextBlock
        {
            Text = def.Url,
            FontSize = 11,
            Opacity = 0.6,
            TextTrimming = TextTrimming.CharacterEllipsis,
        });
        Grid.SetColumn(text, 1);
        grid.Children.Add(text);

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        var add = ActionButton("Add", () =>
        {
            _host.EnableService(def);
            BuildServiceRows();
        });
        add.Margin = new Thickness(6, 0, 0, 0);
        buttons.Children.Add(add);

        if (isCustom)
        {
            var delete = ActionButton("Delete", () => DeleteCustomDefinition(def));
            delete.Margin = new Thickness(6, 0, 0, 0);
            buttons.Children.Add(delete);
        }

        Grid.SetColumn(buttons, 2);
        grid.Children.Add(buttons);
        row.Child = grid;
        return row;
    }

    private void DeleteCustomDefinition(ServiceDefinition def)
    {
        var answer = MessageBox.Show(this,
            $"Remove '{def.DisplayName}' from services.json?\n\nThe definition and its custom logo are deleted. This cannot be undone.",
            "United - remove service", MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (answer != MessageBoxResult.Yes) return;

        CustomServicesFile.Remove(def.Id);
        IconStore.DeleteOverride(def.Id);
        BuildServiceRows();
    }

    private void RemoveCustomService(SidebarItem item)
    {
        var answer = MessageBox.Show(this,
            $"Remove '{item.Def.DisplayName}'?\n\nThe service is disabled, its definition is deleted from services.json and its custom logo is removed.",
            "United - remove service", MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (answer != MessageBoxResult.Yes) return;

        _host.RemoveCustomService(item.Id);
        BuildServiceRows();
    }

    private Button ActionButton(string text, Action onClick)
    {
        var button = new Button
        {
            Content = text,
            Style = (Style)FindResource("SecondaryButton"),
            Padding = new Thickness(10, 4, 10, 4),
            Margin = new Thickness(0, 0, 6, 0),
        };
        button.Click += (_, _) => onClick();
        return button;
    }

    private FrameworkElement BuildIcon(ServiceDefinition def, double size)
    {
        if (IconStore.Resolve(def) is { } image)
        {
            return new Image
            {
                Source = image,
                Width = size,
                Height = size,
                Stretch = Stretch.Uniform,
                VerticalAlignment = VerticalAlignment.Center,
            };
        }

        var path = new System.Windows.Shapes.Path
        {
            Data = MainWindow.LoadIcon(def.Icon),
            Width = size,
            Height = size,
            Stretch = Stretch.Uniform,
            StrokeThickness = 1.4,
            Fill = null,
            VerticalAlignment = VerticalAlignment.Center,
        };
        path.SetResourceReference(System.Windows.Shapes.Shape.StrokeProperty, "TextMuted");
        return path;
    }

    private void UpdateServicesHint()
    {
        ServicesHint.Text =
            $"services.json: {CustomServicesFile.FilePath}\n" +
            $"logos: {IconStore.DirectoryPath}  (PNG/JPG/ICO/BMP/GIF named <service-id>.png; square 128-256 px recommended)";
    }

    private void AddCustom_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new AddServiceWindow { Owner = this };
        if (dialog.ShowDialog() != true) return;

        try
        {
            var def = _host.AddCustomService(dialog.DisplayNameValue, dialog.UrlValue, dialog.LogoPath, dialog.Mode);
            BuildServiceRows();
            ServicesHint.Text = $"Added '{def.DisplayName}' (id {def.Id}) - it is live in the sidebar now.";
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, "add custom service");
            MessageBox.Show(this,
                "The service could not be added.\n\n" + ex.Message,
                "United", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private void SetEnabled(ServiceDefinition def, bool enabled)
    {
        if (_loading) return;
        _host.SetServiceEnabled(def, enabled);
        BuildServiceRows();
    }

    private static bool IsHibernated(SidebarItem item)
        => item.IsDormant || item.Slot.State == ServiceState.Hibernated;

    private void RestartAll_Click(object sender, RoutedEventArgs e)
    {
        foreach (var item in _host.Items) item.Slot.Reload();
    }

    private void OpenServicesJson_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var path = CustomServicesFile.FilePath;
            Directory.CreateDirectory(CustomServicesFile.DirectoryPath);
            if (!File.Exists(path))
            {
                const string template = """
                [
                  {
                    "id": "mychat",
                    "displayName": "My Chat",
                    "url": "https://example.com",
                    "icon": "globe",
                    "originHosts": [ "example.com" ],
                    "titlePatterns": [ "^\\((?<count>\\d+)\\)" ],
                    "enabledByDefault": false
                  }
                ]
                """;
                File.WriteAllText(path, template);
            }
            Process.Start(new ProcessStartInfo(path) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, "open services.json");
            MessageBox.Show(this, ex.Message, "United", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    // ── Performance ────────────────────────────────────────────────

    private void RefreshPerformance()
    {
        try
        {
            var snapshot = _metrics.Capture();
            PerfHostText.Text = $"Host process: {Mb(snapshot.HostPrivateBytes)} private";
            PerfWebText.Text = $"WebView2 tree: {Mb(snapshot.WebView2TreeBytes)} private in {snapshot.WebView2ProcessCount} process(es)";
            PerfCpuText.Text = $"CPU: {snapshot.HostCpuPercent:0.0}% host + {snapshot.WebView2CpuPercent:0.0}% WebView2";
            PerfBrowserPidText.Text = snapshot is { WebView2ProcessCount: > 0 }
                ? $"Shared browser process: pid {_metrics.BrowserProcessId} (one tree for all services)"
                : "Shared browser process: starting…";

            PerfServiceRows.Children.Clear();
            foreach (var item in _host.Items)
            {
                PerfServiceRows.Children.Add(new TextBlock
                {
                    Text = $"{item.Def.DisplayName} — {StateLabel(item.Slot.State)}" +
                           (item.IsDormant ? " (dormant)" : ""),
                    Margin = new Thickness(0, 2, 0, 0),
                });
            }

            // Keep the Services page state labels live without rebuilding the rows.
            foreach (var item in _host.Items)
            {
                if (_stateLabels.TryGetValue(item.Id, out var label))
                    label.Text = $"state {StateLabel(item.Slot.State)}  ·  id {item.Id}";
            }

            RefreshNativeWhatsAppStatus();
        }
        catch
        {
            // metrics are best-effort; keep the window usable
        }
    }

    private static string Mb(long bytes) => $"{bytes / 1024.0 / 1024.0:0.0} MB";

    private static string StateLabel(ServiceState state) => state switch
    {
        ServiceState.Ready => "ready",
        ServiceState.Starting => "starting",
        ServiceState.Suspended => "background (low memory)",
        ServiceState.Hibernated => "hibernated",
        ServiceState.Failed => "failed",
        ServiceState.NotCreated => "not started",
        ServiceState.Disposed => "disposed",
        _ => state.ToString(),
    };

    // ── About ──────────────────────────────────────────────────────

    private void LoadAbout()
    {
        var version = Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0.1.0";
        AboutVersion.Text = $"United Messenger {version}";

        var dataDir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "United");
        AboutPaths.Text =
            $"Settings: {Path.Combine(dataDir, "settings.json")}\n" +
            $"Sessions (profiles): {Path.Combine(dataDir, "WebData")}\n" +
            $"Extra services: {Path.Combine(dataDir, "services.json")}\n" +
            $"Custom logos: {Path.Combine(dataDir, "icons")}\n" +
            $"Logs: {Path.Combine(dataDir, "logs")}";

        try
        {
            var runtime = CoreWebView2Environment.GetAvailableBrowserVersionString(null);
            AboutWebView2.Text = $"WebView2 Evergreen runtime: {runtime}";
        }
        catch (Exception ex)
        {
            AboutWebView2.Text = $"WebView2 runtime: NOT AVAILABLE ({ex.Message})";
        }

        AboutActivation.Text =
            $"Toast AUMID shortcut: {(StartMenuShortcut.ShortcutExists ? "created" : "missing")}\n" +
            $"Protocol {ProtocolRegistration.Scheme}:// : {(ProtocolRegistration.IsRegistered ? "registered (HKCU)" : "not registered")}";

        AutoUpdateCheck.IsChecked = _settings.CheckForUpdatesOnStartup;
        RefreshUpdateStatus();
    }

    private void RefreshUpdateStatus()
    {
        var update = UpdateService.Current;
        var latest = update.Latest;

        if (latest != null)
        {
            UpdateStatusText.Text =
                $"United {latest.Version.ToString(3)} is available. You have {update.CurrentVersionText}.";
            InstallUpdateButton.Visibility = Visibility.Visible;
            return;
        }

        InstallUpdateButton.Visibility = Visibility.Collapsed;
        if (_updateChecked && update.LastError is { Length: > 0 } error)
            UpdateStatusText.Text = $"Update check failed: {error}";
        else if (_updateChecked)
            UpdateStatusText.Text = $"United {update.CurrentVersionText} is up to date.";
        else
            UpdateStatusText.Text = update.IsInstalledBuild
                ? $"United {update.CurrentVersionText}. Check GitHub Releases for a newer installer."
                : $"United {update.CurrentVersionText}. Install with the setup EXE to enable automatic updates.";
    }

    private async void CheckUpdate_Click(object sender, RoutedEventArgs e)
        => await CheckForUpdatesAsync(force: true);

    private async Task CheckForUpdatesAsync(bool force)
    {
        if (_updateBusy) return;
        _updateBusy = true;
        CheckUpdateButton.IsEnabled = false;
        UpdateProgress.Visibility = Visibility.Collapsed;
        UpdateStatusText.Text = "Checking GitHub Releases…";

        try
        {
            await UpdateService.Current.CheckAsync(force);
            _updateChecked = true;
        }
        finally
        {
            _updateBusy = false;
            CheckUpdateButton.IsEnabled = true;
            RefreshUpdateStatus();
        }
    }

    private async void InstallUpdate_Click(object sender, RoutedEventArgs e)
    {
        var update = UpdateService.Current;
        var release = update.Latest;
        if (release is null)
        {
            await CheckForUpdatesAsync(force: true);
            release = update.Latest;
            if (release is null) return;
        }
        if (!update.IsInstalledBuild)
        {
            MessageBox.Show(this,
                "Automatic installation is available for the installed build.\n\n" +
                "Download the latest installer from the Releases page.",
                "United update", MessageBoxButton.OK, MessageBoxImage.Information);
            Process.Start(new ProcessStartInfo(release.ReleaseUrl) { UseShellExecute = true });
            return;
        }

        var answer = MessageBox.Show(this,
            $"Download and install United {release.Version.ToString(3)}?\n\n" +
            "United will close and reopen when the installer finishes.",
            "United update", MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (answer != MessageBoxResult.Yes) return;

        _updateBusy = true;
        CheckUpdateButton.IsEnabled = false;
        InstallUpdateButton.IsEnabled = false;
        UpdateProgress.Visibility = Visibility.Visible;
        UpdateProgress.Value = 0;

        try
        {
            var progress = new Progress<double>(value =>
            {
                UpdateProgress.Value = value;
                UpdateStatusText.Text = $"Downloading United {release.Version.ToString(3)}… {value * 100:0}%";
            });

            var installer = await update.DownloadAndVerifyAsync(release, progress);
            UpdateStatusText.Text = "Checksum verified. Installing update — United will reopen automatically…";

            if (update.BeginInstall(installer))
            {
                if (Application.Current is App app) app.RequestExit();
                else Application.Current.Shutdown();
            }
            else
            {
                UpdateStatusText.Text = "Could not start the installer. Opening the Releases page…";
                Process.Start(new ProcessStartInfo(release.ReleaseUrl) { UseShellExecute = true });
            }
        }
        catch (Exception ex)
        {
            UpdateStatusText.Text = $"Update failed: {ex.Message}";
            MessageBox.Show(this,
                $"The update could not be installed.\n\n{ex.Message}\n\n" +
                "You can download the latest installer from the Releases page.",
                "United update", MessageBoxButton.OK, MessageBoxImage.Error);
        }
        finally
        {
            _updateBusy = false;
            CheckUpdateButton.IsEnabled = true;
            InstallUpdateButton.IsEnabled = true;
            UpdateProgress.Visibility = Visibility.Collapsed;
        }
    }

    private void UpdateSetting_Changed(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        _settings.CheckForUpdatesOnStartup = AutoUpdateCheck.IsChecked == true;
        _store.Save(_settings);
    }

    private void SupportUnited_Click(object sender, RoutedEventArgs e)
        => Process.Start(new ProcessStartInfo(AppLinks.Support) { UseShellExecute = true });

    private void OpenGitHub_Click(object sender, RoutedEventArgs e)
        => Process.Start(new ProcessStartInfo(AppLinks.GitHub) { UseShellExecute = true });

    private void OpenDataFolder_Click(object sender, RoutedEventArgs e)
    {
        var dir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "United");
        Directory.CreateDirectory(dir);
        Process.Start(new ProcessStartInfo(dir) { UseShellExecute = true });
    }

    private void OpenLogs_Click(object sender, RoutedEventArgs e)
    {
        Directory.CreateDirectory(AppLog.Directory_);
        Process.Start(new ProcessStartInfo(AppLog.Directory_) { UseShellExecute = true });
    }
}
