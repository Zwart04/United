using System.Windows;
using Microsoft.Win32;
using United.App.Diagnostics;
using United.App.Shell;
using United.Core.Services;

namespace United.App.Views;

/// <summary>Collects the name, address, optional logo and notification mode for a custom service.</summary>
public partial class AddServiceWindow : Window
{
    public AddServiceWindow()
    {
        InitializeComponent();

        ModeCombo.Items.Add("Host - app toast (recommended)");
        ModeCombo.Items.Add("Webview - the site's own notification");
        ModeCombo.Items.Add("Off");
        ModeCombo.SelectedIndex = 0;

        Loaded += (_, _) => NameBox.Focus();
    }

    public string DisplayNameValue { get; private set; } = "";

    public string UrlValue { get; private set; } = "";

    public string? LogoPath { get; private set; }

    public NotificationMode Mode => (NotificationMode)ModeCombo.SelectedIndex;

    private void ChooseLogo_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = "Choose a service logo",
            Filter = IconStore.FileDialogFilter,
            CheckFileExists = true,
        };
        if (dialog.ShowDialog(this) != true) return;

        try
        {
            var image = IconStore.LoadImage(dialog.FileName);
            image.Freeze();
            LogoPreview.Source = image;
            LogoPreview.Visibility = Visibility.Visible;
            LogoGlyph.Visibility = Visibility.Collapsed;
            LogoPath = dialog.FileName;
            ErrorText.Text = "";
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"logo preview {dialog.FileName}");
            ErrorText.Text = "That image could not be read. Choose a PNG, JPG, ICO, BMP or GIF file.";
            LogoPath = null;
        }
    }

    private void ClearLogo_Click(object sender, RoutedEventArgs e)
    {
        LogoPath = null;
        LogoPreview.Source = null;
        LogoPreview.Visibility = Visibility.Collapsed;
        LogoGlyph.Visibility = Visibility.Visible;
    }

    private void Add_Click(object sender, RoutedEventArgs e)
    {
        var name = NameBox.Text.Trim();
        if (name.Length < 2)
        {
            ErrorText.Text = "Enter a name (at least 2 characters).";
            NameBox.Focus();
            return;
        }

        var raw = UrlBox.Text.Trim();
        if (raw.Length == 0)
        {
            ErrorText.Text = "Enter the website address, for example https://example.com.";
            UrlBox.Focus();
            return;
        }

        if (!raw.Contains("://", StringComparison.Ordinal)) raw = "https://" + raw;
        if (!Uri.TryCreate(raw, UriKind.Absolute, out var uri) ||
            (uri.Scheme != Uri.UriSchemeHttps && uri.Scheme != Uri.UriSchemeHttp) ||
            string.IsNullOrWhiteSpace(uri.Host))
        {
            ErrorText.Text = "That address is not valid. Use a full http(s) URL, for example https://example.com/messages.";
            UrlBox.Focus();
            return;
        }

        DisplayNameValue = name;
        UrlValue = uri.ToString();
        DialogResult = true;
    }
}
