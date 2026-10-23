using System.Diagnostics;
using System.IO;
using System.Windows;
using System.Windows.Threading;
using United.App.Diagnostics;
using United.App.Services;
using United.App.Shell;
using United.Core.Settings;
using United.Platform.Windows.Activation;
using United.Platform.Windows.Metrics;
using United.Platform.Windows.Notifications;
using United.Platform.Windows.Startup;
using United.Platform.Windows.SystemThemes;
using United.Platform.Windows.Tray;

namespace United.App;

public partial class App : Application
{
    private SingleInstance? _single;
    private SettingsStore? _store;
    private AppSettings? _settings;
    private MainWindow? _window;
    private TrayIconService? _tray;
    private ThemeManager? _theme;
    private ThemeWatcher? _watcher;
    private ToastService? _toasts;
    private ProcessMetricsCollector? _metrics;
    private DispatcherTimer? _pidTimer;
    private DispatcherTimer? _updateTimer;

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        AppLog.Info($"United starting; args: [{string.Join(" | ", e.Args)}]");

        // ── Single instance: one UDF can only be served by one active environment. ──
        _single = new SingleInstance();
        if (!_single.IsPrimary)
        {
            var payload = e.Args.FirstOrDefault(a =>
                              a.StartsWith(ProtocolRegistration.Scheme + "://", StringComparison.OrdinalIgnoreCase))
                          ?? "activate";
            SingleInstance.ForwardToPrimary(payload);
            _single.Dispose();
            Shutdown(0);
            return;
        }

        // Log and keep the session alive on UI-thread exceptions; the user can still quit from the tray.
        DispatcherUnhandledException += (_, ex) =>
        {
            AppLog.Exception(ex.Exception, "unhandled UI exception");
            ex.Handled = true;
        };

        var exe = Environment.ProcessPath
                  ?? Path.Combine(AppContext.BaseDirectory, "United.exe");

        // ── Platform registrations (per user, no admin) ──
        var shortcutOk = StartMenuShortcut.Ensure(exe);
        ProtocolRegistration.EnsureRegistered(exe);

        // ── Settings / theme / services ──
        _store = new SettingsStore(SettingsStore.DefaultPath);
        _settings = _store.Load();
        _theme = new ThemeManager();
        _theme.Apply(_settings.Theme);
        _watcher = new ThemeWatcher();
        _watcher.ThemeChanged += t => Dispatcher.BeginInvoke(() =>
        {
            if (string.Equals(_settings?.Theme, "system", StringComparison.OrdinalIgnoreCase))
                _theme?.ApplyResolved(t);
        });
        _metrics = new ProcessMetricsCollector();
        _toasts = new ToastService(shortcutOk);
        _toasts.ToastFailed += msg => AppLog.Warn($"toast failure: {msg}");

        // ── Main window ──
        _window = new MainWindow(_settings, _store, _theme, _watcher, _metrics, _toasts);
        _window.UnreadTotalChanged += total =>
            _tray?.SetBadge(_settings!.ShowTrayBadge ? total : 0);

        // Show the shell first (loading states are visible), then connect the services.
        var protocolArg0 = e.Args.FirstOrDefault(a =>
            a.StartsWith(ProtocolRegistration.Scheme + "://", StringComparison.OrdinalIgnoreCase));
        var startMinimized0 = _settings.StartMinimized || e.Args.Contains("--minimized");
        if (!startMinimized0 || protocolArg0 != null) ShowMainWindow();

        await _window.InitializeAsync();

        // ── Tray ──
        _tray = new TrayIconService();
        _tray.OpenRequested += () => Dispatcher.BeginInvoke(ShowMainWindow);
        _tray.SettingsRequested += () => Dispatcher.BeginInvoke(() => _window?.OpenSettings());
        _tray.SupportRequested += () => Dispatcher.BeginInvoke(() =>
            Process.Start(new ProcessStartInfo(AppLinks.Support) { UseShellExecute = true }));
        _tray.HibernateAllRequested += () => Dispatcher.BeginInvoke(() => _window?.HibernateAll());
        _tray.QuitRequested += () => Dispatcher.BeginInvoke(RequestExit);

        // Feed the shared browser pid into the metrics collector (proof of one browser tree).
        _pidTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(2) };
        _pidTimer.Tick += (_, _) =>
        {
            var pid = _window?.Items.FirstOrDefault(i => i.Slot.IsLive)?.Slot.BrowserProcessId ?? 0;
            if (pid > 0) _metrics!.SetBrowserProcessId(pid);
        };
        _pidTimer.Start();

        // ── Activation payload (cold start via united:// or --minimized autostart) ──
        var protocolArg = e.Args.FirstOrDefault(a =>
            a.StartsWith(ProtocolRegistration.Scheme + "://", StringComparison.OrdinalIgnoreCase));
        if (protocolArg != null && ProtocolRegistration.ParseOpenRequest(protocolArg) is { } serviceId)
            ActivateServiceOrSettings(serviceId);

        // Settings file is the source of truth for autostart.
        if (Autostart.IsEnabled != _settings.LaunchAtStartup && Environment.ProcessPath is { } p)
            Autostart.SetEnabled(_settings.LaunchAtStartup, p, _settings.StartMinimized);

        // Forwarded message pipe (secondary launches: toast clicks, second exe run).
        _single.MessageReceived += msg => Dispatcher.BeginInvoke(() => HandleActivationMessage(msg));
        _single.StartServer();

        ScheduleUpdateCheck();

        AppLog.Info($"startup complete (shortcut={shortcutOk})");
    }

    private void HandleActivationMessage(string message)
    {
        if (ProtocolRegistration.ParseOpenRequest(message) is { } serviceId)
        {
            ShowMainWindow();
            ActivateServiceOrSettings(serviceId);
        }
        else
        {
            ShowMainWindow();
        }
    }

    private void ActivateServiceOrSettings(string serviceId)
    {
        if (string.Equals(serviceId, "settings", StringComparison.OrdinalIgnoreCase))
        {
            _window?.OpenSettings();
            return;
        }

        _window?.ActivateService(serviceId);
    }

    /// <summary>
    /// Checks GitHub Releases 12 seconds after startup, at most once a day. An
    /// available update raises a toast whose click opens Settings › About.
    /// </summary>
    private void ScheduleUpdateCheck()
    {
        if (_settings?.CheckForUpdatesOnStartup != true) return;
        if (_settings.LastUpdateCheckUtc is { } last &&
            DateTimeOffset.UtcNow - last < TimeSpan.FromHours(23)) return;

        _updateTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(12) };
        _updateTimer.Tick += async (_, _) =>
        {
            _updateTimer?.Stop();
            try
            {
                var result = await UpdateService.Current.CheckAsync();
                if (result.Error == null)
                {
                    _settings!.LastUpdateCheckUtc = DateTimeOffset.UtcNow;
                    _store?.Save(_settings);
                }

                if (result.UpdateAvailable && UpdateService.Current.Latest is { } release)
                {
                    AppLog.Info($"startup update check: {release.Version.ToString(3)} available");
                    _toasts?.Show("settings", "United update available",
                        $"Version {release.Version.ToString(3)} is ready. Open Settings > About to install.",
                        sound: false);
                }
            }
            catch (Exception ex)
            {
                AppLog.Exception(ex, "startup update check");
            }
        };
        _updateTimer.Start();
    }

    private void ShowMainWindow()
    {
        if (_window == null) return;
        if (_window.WindowState == WindowState.Minimized) _window.WindowState = WindowState.Normal;
        _window.Show();
        _window.Activate();
    }

    /// <summary>Real exit (tray Quit / close with tray disabled): tear down webviews, tray, pipe.</summary>
    public void RequestExit()
    {
        AppLog.Info("exit requested");
        try { _pidTimer?.Stop(); } catch { /* best effort */ }
        try { _updateTimer?.Stop(); } catch { /* best effort */ }
        try { _window?.PrepareExit(); } catch (Exception ex) { AppLog.Exception(ex, "window prepare exit"); }
        try { _tray?.Dispose(); } catch { /* best effort */ }
        try { _toasts?.Dispose(); } catch { /* best effort */ }
        try { _watcher?.Dispose(); } catch { /* best effort */ }
        try { _single?.Dispose(); } catch { /* best effort */ }
        Shutdown(0);
    }

    protected override void OnExit(ExitEventArgs e)
    {
        AppLog.Info("exit");
        base.OnExit(e);
    }
}
