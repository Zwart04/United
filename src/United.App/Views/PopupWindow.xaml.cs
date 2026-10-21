using System.Windows;
using United.App.Diagnostics;
using United.App.Web;
using United.Core.Services;
using Microsoft.Web.WebView2.Core;

namespace United.App.Views;

/// <summary>
/// Lightweight popup window for in-scope popups (OAuth / secondary flows).
/// It uses the SAME profile as the originating service, so cookies and login
/// state are shared — signing in here completes the flow in the main webview.
/// </summary>
public partial class PopupWindow : Window
{
    private readonly ServiceDefinition _def;
    private readonly CoreWebView2Environment _env;
    private readonly string _uri;
    private bool _closing;
    private bool _dark;

    public PopupWindow(ServiceDefinition def, CoreWebView2Environment env, string uri, Window owner)
    {
        _def = def;
        _env = env;
        _uri = uri;
        IsAuthFlow = ServiceSlot.IsAuthHost(uri);
        Owner = owner;
        Title = def.DisplayName;
        InitializeComponent();
        Loaded += OnLoaded;
        Closed += (_, _) =>
        {
            _closing = true;
            try { Web.Dispose(); } catch { /* best effort */ }
        };
    }

    /// <summary>True when this popup participated in an OAuth/provider sign-in.</summary>
    public bool IsAuthFlow { get; private set; }

    public void ApplyTheme(bool dark)
    {
        _dark = dark;
        try
        {
            Web.DefaultBackgroundColor = dark
                ? System.Drawing.Color.FromArgb(0x11, 0x1B, 0x21)
                : System.Drawing.Color.FromArgb(0xF5, 0xF5, 0xF5);

            var c = Web.CoreWebView2;
            if (c != null)
            {
                c.Profile.PreferredColorScheme = dark
                    ? CoreWebView2PreferredColorScheme.Dark
                    : CoreWebView2PreferredColorScheme.Light;
            }
        }
        catch { /* best effort while the controller is initialising */ }
    }

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        Loaded -= OnLoaded;
        try
        {
            var options = _env.CreateCoreWebView2ControllerOptions();
            options.ProfileName = _def.Id.ToLowerInvariant();
            await Web.EnsureCoreWebView2Async(_env, options);
            if (_closing) return;

            var c = Web.CoreWebView2;
            ApplyTheme(_dark);
            c.Settings.IsStatusBarEnabled = false;
            c.PermissionRequested += (_, pe) => PermissionBroker.Handle(_def, pe);
            c.NavigationStarting += (_, e) =>
            {
                if (ServiceSlot.IsAuthHost(e.Uri)) IsAuthFlow = true;
            };
            c.NewWindowRequested += (_, ne) =>
            {
                ne.Handled = true;
                if (ServiceSlot.IsExternal(_def, ne.Uri) && !ServiceSlot.IsAuthHost(ne.Uri))
                    ServiceSlot.OpenInSystemBrowser(ne.Uri);
                else
                    c.Navigate(ne.Uri);
            };
            c.DocumentTitleChanged += (_, _) => Dispatcher.Invoke(() =>
                Title = string.IsNullOrWhiteSpace(c.DocumentTitle)
                    ? _def.DisplayName
                    : $"{c.DocumentTitle} — {_def.DisplayName}");

            c.Navigate(_uri);
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"popup init for {_def.Id}");
            Close();
        }
    }
}
