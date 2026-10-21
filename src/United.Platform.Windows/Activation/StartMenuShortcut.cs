using System.Runtime.InteropServices;
using United.Platform.Windows.Interop;

namespace United.Platform.Windows.Activation;

/// <summary>
/// Creates/repairs the Start menu shortcut carrying the app's AppUserModelID.
/// Windows requires an AUMID-associated shortcut for classic (unpackaged) apps
/// to display toast notifications attributed to the app.
/// </summary>
public static class StartMenuShortcut
{
    public const string Aumid = "United.Messenger";
    public const string ShortcutDisplayName = "United Messenger";

    private static string? _shortcutPath;
    private static bool _ensured;

    public static string ShortcutPath =>
        _shortcutPath ??= Path.Combine(
            NativeMethods.KnownFolder(0x0002), "United Messenger", ShortcutDisplayName + ".lnk");

    /// <summary>Creates the shortcut if missing or pointing somewhere else. Never throws.</summary>
    public static bool Ensure(string exePath)
    {
        if (_ensured && File.Exists(ShortcutPath)) return true;
        try
        {
            var dir = Path.GetDirectoryName(ShortcutPath)!;
            Directory.CreateDirectory(dir);

            object shellLinkObj = Activator.CreateInstance(Type.GetTypeFromCLSID(NativeMethods.CLSID_ShellLink)!)!;
            try
            {
                var link = (IShellLinkW)shellLinkObj;
                link.SetPath(exePath);
                link.SetWorkingDirectory(Path.GetDirectoryName(exePath) ?? "");
                link.SetDescription("Unified messenger — WhatsApp, Instagram, Telegram, Discord, Threads in one window");
                link.SetIconLocation(exePath, 0);

                var persist = (IPersistFile)shellLinkObj;
                persist.Save(ShortcutPath, true);

                // Attach AppUserModelID so toasts attribute to this shortcut.
                if (shellLinkObj is IPropertyStore store)
                {
                    var key = new NativeMethods.PROPERTYKEY
                    {
                        fmtid = NativeMethods.PKEY_AppUserModel_ID_Format,
                        pid = 5,
                    };
                    var pv = PROPVARIANT.FromString(Aumid);
                    try
                    {
                        store.SetValue(ref key, ref pv);
                        store.Commit();
                    }
                    finally
                    {
                        pv.Free();
                    }
                }

                _ensured = true;
                return true;
            }
            finally
            {
                Marshal.FinalReleaseComObject(shellLinkObj);
            }
        }
        catch (Exception)
        {
            // Shortcut creation is best-effort; toasts are still attempted (they may show
            // attributed to the executable name). The failure surfaces in diagnostics.
            return false;
        }
    }

    public static bool ShortcutExists => File.Exists(ShortcutPath);
}
