using System.IO;
using System.Windows;
using System.Windows.Threading;
using United.App.Diagnostics;
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
            _window.ActivateService(serviceId);

        // Settings file is the source of truth for autostart.
        if (Autostart.IsEnabled != _settings.LaunchAtStartup && Environment.ProcessPath is { } p)
            Autostart.SetEnabled(_settings.LaunchAtStartup, p, _settings.StartMinimized);

        // Forwarded message pipe (secondary launches: toast clicks, second exe run).
        _single.MessageReceived += msg => Dispatcher.BeginInvoke(() => HandleActivationMessage(msg));
        _single.StartServer();

        AppLog.Info($"startup complete (shortcut={shortcutOk})");
    }

    private void HandleActivationMessage(string message)
    {
        if (ProtocolRegistration.ParseOpenRequest(message) is { } serviceId)
        {
            ShowMainWindow();
            _window?.ActivateService(serviceId);
        }
        else
        {
            ShowMainWindow();
        }
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
