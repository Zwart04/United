using Microsoft.Web.WebView2.Core;
using United.App.Diagnostics;
using United.Core.Services;

namespace United.App.Web;

/// <summary>
/// Central permission policy for embedded web content.
/// Remote sites run only inside the Chromium sandbox; they never receive direct
/// bridges to native APIs. Permissions are granted narrowly and declaratively.
/// </summary>
public static class PermissionBroker
{
    public static void Handle(ServiceDefinition definition, CoreWebView2PermissionRequestedEventArgs e)
    {
        var host = Uri.TryCreate(e.Uri, UriKind.Absolute, out var uri) ? uri.Host : "";
        var ownHost = host.Length > 0 && definition.OriginHosts.Any(h =>
            host.Equals(h, StringComparison.OrdinalIgnoreCase) ||
            host.EndsWith("." + h, StringComparison.OrdinalIgnoreCase));

        var allow = e.PermissionKind switch
        {
            CoreWebView2PermissionKind.Notifications => true,
            // IndexedDB/localStorage: sessions depend on it.
            CoreWebView2PermissionKind.PersistentStorage => true,
            // Calls need these, but only from the service's own origin. An embedded
            // third-party frame cannot turn the app into a camera/microphone proxy.
            CoreWebView2PermissionKind.Camera => ownHost,
            CoreWebView2PermissionKind.Microphone => ownHost,
            // Ctrl+V paste does not need this; silent navigator.clipboard.readText()
            // would let any page read whatever was copied from another app.
            CoreWebView2PermissionKind.ClipboardRead => false,
            _ => false,
        };

        e.State = allow ? CoreWebView2PermissionState.Allow : CoreWebView2PermissionState.Deny;
        if (host.Length == 0) host = "unknown";
        AppLog.Info($"permission [{definition.Id}] {e.PermissionKind} from {host}: {(allow ? "allow" : "deny")}");
    }
}
