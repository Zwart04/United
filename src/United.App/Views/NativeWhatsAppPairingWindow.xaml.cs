using System.IO;
using System.Windows;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using United.App.Web;

namespace United.App.Views;

public partial class NativeWhatsAppPairingWindow : Window
{
    private readonly NativeWhatsAppBridge _bridge;
    private readonly DispatcherTimer _timer;
    private DateTime _lastQrWrite = DateTime.MinValue;

    public NativeWhatsAppPairingWindow(NativeWhatsAppBridge bridge)
    {
        _bridge = bridge;
        InitializeComponent();

        _timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(900) };
        _timer.Tick += (_, _) => RefreshState();
        Loaded += (_, _) =>
        {
            _bridge.Start();
            _timer.Start();
            RefreshState();
        };
        Closed += (_, _) => _timer.Stop();
    }

    private void RefreshState()
    {
        if (!_bridge.IsAvailable)
        {
            StatusText.Text = "United.WhatsAppBridge.exe is missing from this build.";
            QrPlaceholder.Text = "Native bridge not available";
            QrPlaceholder.Visibility = Visibility.Visible;
            QrImage.Visibility = Visibility.Collapsed;
            return;
        }

        var status = _bridge.Status;
        switch (status.State)
        {
            case "connected":
                StatusText.Text = "Connected. Native WhatsApp is collecting messages.";
                HintText.Text =
                    "You can close this window. The WhatsApp WebView will stay asleep until you open a chat.";
                QrImage.Visibility = Visibility.Collapsed;
                QrPlaceholder.Text = "Connected";
                QrPlaceholder.Visibility = Visibility.Visible;
                return;
            case "error":
                StatusText.Text = "Native bridge error: " + (status.Error ?? "unknown");
                QrImage.Visibility = Visibility.Collapsed;
                QrPlaceholder.Text = "Native bridge error";
                QrPlaceholder.Visibility = Visibility.Visible;
                return;
            case "pairing":
                StatusText.Text = "Waiting for the QR scan on your phone.";
                break;
            case "pairing_timeout":
                StatusText.Text = "The QR code expired. Click Refresh QR to generate a new one.";
                QrImage.Visibility = Visibility.Collapsed;
                QrPlaceholder.Text = "QR code expired";
                QrPlaceholder.Visibility = Visibility.Visible;
                return;
            case "disconnected":
                StatusText.Text = "Paired, reconnecting...";
                break;
            case "logged_out":
                StatusText.Text = "The phone unlinked this device. Pair again to resume native collection.";
                break;
            default:
                StatusText.Text = status.Paired ? "Paired, waiting for connection..." : "Starting the native bridge...";
                break;
        }

        if (status.State != "pairing")
        {
            QrImage.Visibility = Visibility.Collapsed;
            QrPlaceholder.Text = status.Paired ? "Paired" : "Waiting for the bridge...";
            QrPlaceholder.Visibility = Visibility.Visible;
            return;
        }

        if (File.Exists(_bridge.QrPath))
        {
            var write = File.GetLastWriteTimeUtc(_bridge.QrPath);
            if (write != _lastQrWrite)
            {
                try
                {
                    // Read fully first: the bridge may be replacing the file while
                    // the timer ticks.
                    var bytes = File.ReadAllBytes(_bridge.QrPath);
                    using var stream = new MemoryStream(bytes);
                    var image = new BitmapImage();
                    image.BeginInit();
                    image.CacheOption = BitmapCacheOption.OnLoad;
                    image.StreamSource = stream;
                    image.EndInit();
                    image.Freeze();
                    QrImage.Source = image;
                    QrImage.Visibility = Visibility.Visible;
                    QrPlaceholder.Visibility = Visibility.Collapsed;
                    _lastQrWrite = write;
                }
                catch
                {
                    // A partially written PNG is retried on the next tick.
                }
            }
        }
        else
        {
            QrImage.Visibility = Visibility.Collapsed;
            QrPlaceholder.Text = "Preparing QR code...";
            QrPlaceholder.Visibility = Visibility.Visible;
        }
    }

    private void Refresh_Click(object sender, RoutedEventArgs e)
    {
        if (_bridge.IsConnected) return;
        _lastQrWrite = DateTime.MinValue;
        _bridge.Stop();
        _bridge.Start();
        RefreshState();
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();
}
