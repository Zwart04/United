using System.Runtime.InteropServices;
using System.Text;

namespace United.Platform.Windows.Interop;

/// <summary>P/Invoke surface shared by the platform layer and the app shell.</summary>
public static class NativeMethods
{
    // ---- Toolhelp32 process snapshot ----
    public const uint TH32CS_SNAPPROCESS = 0x00000002;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct PROCESSENTRY32
    {
        public uint dwSize;
        public uint cntUsage;
        public uint th32ProcessID;
        public IntPtr th32DefaultHeapID;
        public uint th32ModuleID;
        public uint cntThreads;
        public uint th32ParentProcessID;
        public int pcPriClassBase;
        public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string szExeFile;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr CreateToolhelp32Snapshot(uint dwFlags, uint th32ProcessID);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool Process32First(IntPtr hSnapshot, ref PROCESSENTRY32 lppe);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool Process32Next(IntPtr hSnapshot, ref PROCESSENTRY32 lppe);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);

    public static readonly IntPtr InvalidHandleValue = new(-1);

    // ---- Shell link (Start menu shortcut with AppUserModelID) ----
    public const int STGM_READWRITE = 0x00000002;
    public const int STGM_SHARE_DENY_NONE = 0x00000040;

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern int SHGetKnownFolderPath([MarshalAs(UnmanagedType.LPStruct)] Guid rfid, uint dwFlags, IntPtr hToken, out IntPtr ppszPath);

    [DllImport("ole32.dll")]
    public static extern int CoCreateInstance(ref Guid rclsid, IntPtr pUnkOuter, uint dwClsContext, ref Guid riid, out IntPtr ppv);

    [DllImport("ole32.dll")]
    public static extern void CoTaskMemFree(IntPtr pv);

    public static readonly Guid CLSID_ShellLink = new("00021401-0000-0000-C000-000000000046");
    public static readonly Guid IID_IShellLinkW = new("000214F9-0000-0000-C000-000000000046");
    public static readonly Guid IID_IPersistFile = new("0000010b-0000-0000-C000-000000000046");
    public static readonly Guid IID_IPropertyStore = new("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99");

    /// <summary>PKEY_AppUserModel_ID — {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3} pid 5</summary>
    public static readonly Guid PKEY_AppUserModel_ID_Format = new("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3");

    [StructLayout(LayoutKind.Sequential)]
    public struct PROPERTYKEY
    {
        public Guid fmtid;
        public uint pid;
    }

    // ---- DWM (dark title bar on Win10 19041+ / Win11) ----
    [DllImport("dwmapi.dll")]
    public static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int pv, int cbSize);

    public const int DWMWA_USE_IMMERSIVE_DARK_MODE = 20;

    // ---- Monitor (maximize must stay inside the work area) ----
    public const uint MONITOR_DEFAULTTONEAREST = 2;

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int left, top, right, bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct MONITORINFO
    {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public int dwFlags;
    }

    [DllImport("user32.dll")]
    public static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint dwFlags);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFO lpmi);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    public const int SW_RESTORE = 9;

    [DllImport("user32.dll")]
    public static extern IntPtr FindWindow(string? lpClassName, string lpWindowName);

    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr hIcon);

    // ---- Process memory (psapi) ----
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct PROCESS_MEMORY_COUNTERS_EX
    {
        public uint cb;
        public uint PageFaultCount;
        public UIntPtr PeakWorkingSetSize;
        public UIntPtr WorkingSetSize;
        public UIntPtr QuotaPeakPagedPoolUsage;
        public UIntPtr QuotaPagedPoolUsage;
        public UIntPtr QuotaPeakNonPagedPoolUsage;
        public UIntPtr QuotaNonPagedPoolUsage;
        public UIntPtr PagefileUsage;
        public UIntPtr PeakPagefileUsage;
        public UIntPtr PrivateUsage; // private working set bytes
    }

    [DllImport("psapi.dll", SetLastError = true)]
    public static extern bool GetProcessMemoryInfo(IntPtr hProcess, ref PROCESS_MEMORY_COUNTERS_EX ppsmemCounters, uint cb);

    public static string KnownFolder(int knownFolderId)
    {
        // CSIDL_PROGRAMS = 0x0002 → GUID for FOLDERID_Programs
        var guid = new Guid("52A7F2DA-9DC3-47BE-A625-B54859D3A9D4"); // FOLDERID_Programs
        if (SHGetKnownFolderPath(guid, 0, IntPtr.Zero, out var p) == 0 && p != IntPtr.Zero)
        {
            var s = Marshal.PtrToStringUni(p);
            CoTaskMemFree(p);
            if (!string.IsNullOrEmpty(s)) return s;
        }
        return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.StartMenu),
            "Programs");
    }

    public static void ThrowIfFailed(int hr) => Marshal.ThrowExceptionForHR(hr);
}

/// <summary>Minimal COM vtable declarations for IShellLinkW / IPersistFile / IPropertyStore.</summary>
[ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IShellLinkW
{
    void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszFile, int cch, IntPtr pfd, uint fFlags);
    void GetIDList(out IntPtr ppidl);
    void SetIDList(IntPtr pidl);
    void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszName, int cch);
    void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string pszName);
    void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszDir, int cch);
    void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string pszDir);
    void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszArgs, int cch);
    void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string pszArgs);
    void GetHotkey(out short pwHotkey);
    void SetHotkey(short wHotkey);
    void GetShowCmd(out int piShowCmd);
    void SetShowCmd(int iShowCmd);
    void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszIconPath, int cch, out int piIcon);
    void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string pszIconPath, int iIcon);
    void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string pszPath, uint dwReserved);
    void Resolve(IntPtr hwnd, uint fFlags);
    void SetPath([MarshalAs(UnmanagedType.LPWStr)] string pszFile);
}

[ComImport, Guid("0000010b-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IPersistFile
{
    void GetClassID(out Guid pClassID);
    void IsDirty();
    void Load([MarshalAs(UnmanagedType.LPWStr)] string pszFileName, uint dwMode);
    void Save([MarshalAs(UnmanagedType.LPWStr)] string pszFileName, [MarshalAs(UnmanagedType.Bool)] bool fRemember);
    void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string pszFileName);
    void GetCurFile(out IntPtr ppszFileName);
}

[ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IPropertyStore
{
    void GetCount(out uint cProps);
    void GetAt(uint iProp, out NativeMethods.PROPERTYKEY pkey);
    void GetValue(ref NativeMethods.PROPERTYKEY key, out PROPVARIANT pv);
    void SetValue(ref NativeMethods.PROPERTYKEY key, ref PROPVARIANT propvar);
    void Commit();
}

[StructLayout(LayoutKind.Sequential)]
internal struct PROPVARIANT
{
    public ushort vt;
    public ushort wReserved1;
    public ushort wReserved2;
    public ushort wReserved3;
    public IntPtr p;
    public int p2;

    public static PROPVARIANT FromString(string value)
    {
        // VT_LPWSTR = 31
        return new PROPVARIANT
        {
            vt = 31,
            p = Marshal.StringToCoTaskMemUni(value),
        };
    }

    public void Free()
    {
        if (p != IntPtr.Zero) Marshal.FreeCoTaskMem(p);
        p = IntPtr.Zero;
    }
}
