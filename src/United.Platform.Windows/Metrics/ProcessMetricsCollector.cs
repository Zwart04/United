using System.Diagnostics;
using United.Platform.Windows.Interop;

namespace United.Platform.Windows.Metrics;

public sealed record MemorySnapshot(
    long HostPrivateBytes,
    long WebView2TreeBytes,
    int WebView2ProcessCount,
    double HostCpuPercent,
    double WebView2CpuPercent);

/// <summary>
/// Measures the host process plus every msedgewebview2.exe process descended from the
/// shared WebView2 browser process (private working set — no shared-page double counting).
/// </summary>
public sealed class ProcessMetricsCollector
{
    private int _browserPid;
    private long _lastHostCpu;
    private long _lastTreeCpu;
    private DateTime _lastSampleUtc;

    public void SetBrowserProcessId(int pid)
    {
        if (pid > 0) _browserPid = pid;
    }

    public int BrowserProcessId => _browserPid;

    public MemorySnapshot Capture()
    {
        var now = DateTime.UtcNow;

        long hostBytes = 0;
        try
        {
            using var self = Process.GetCurrentProcess();
            hostBytes = PrivateBytes(self);
        }
        catch { /* process query can fail during shutdown */ }

        long treeBytes = 0;
        int count = 0;
        long hostCpu = 0, treeCpu = 0;

        try
        {
            using var self = Process.GetCurrentProcess();
            hostCpu = self.TotalProcessorTime.Ticks;
        }
        catch { /* ignore */ }

        var pids = _browserPid > 0 ? CollectDescendants(_browserPid) : new HashSet<int>();
        foreach (var pid in pids)
        {
            try
            {
                using var p = Process.GetProcessById(pid);
                if (!p.ProcessName.StartsWith("msedgewebview2", StringComparison.OrdinalIgnoreCase) &&
                    pid != _browserPid)
                    continue;
                treeBytes += PrivateBytes(p);
                treeCpu += p.TotalProcessorTime.Ticks;
                count++;
            }
            catch { /* process exited between snapshot and query */ }
        }

        double hostPct = 0, treePct = 0;
        if (_lastSampleUtc != default)
        {
            var elapsed = now - _lastSampleUtc;
            if (elapsed > TimeSpan.FromMilliseconds(200))
            {
                var cpuCount = Math.Max(1, Environment.ProcessorCount);
                hostPct = ClampPercent((hostCpu - _lastHostCpu) / (double)elapsed.Ticks * 100 / cpuCount);
                treePct = ClampPercent((treeCpu - _lastTreeCpu) / (double)elapsed.Ticks * 100 / cpuCount);
            }
        }

        _lastSampleUtc = now;
        _lastHostCpu = hostCpu;
        _lastTreeCpu = treeCpu;

        return new MemorySnapshot(hostBytes, treeBytes, count, hostPct, treePct);
    }

    private static double ClampPercent(double v) => v < 0 ? 0 : (v > 100 ? 100 : v);

    /// <summary>Private working set bytes (psapi). On .NET, Process.PrivateWorkingSet64 does not exist.</summary>
    private static long PrivateBytes(Process p)
    {
        var counters = new NativeMethods.PROCESS_MEMORY_COUNTERS_EX
        {
            cb = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.PROCESS_MEMORY_COUNTERS_EX>(),
        };
        return NativeMethods.GetProcessMemoryInfo(p.Handle, ref counters, counters.cb)
            ? (long)counters.PrivateUsage
            : 0;
    }

    /// <summary>All descendants of <paramref name="rootPid"/> from a Toolhelp snapshot.</summary>
    public static HashSet<int> CollectDescendants(int rootPid)
    {
        var result = new HashSet<int> { rootPid };
        var snapshot = NativeMethods.CreateToolhelp32Snapshot(NativeMethods.TH32CS_SNAPPROCESS, 0);
        if (snapshot == IntPtr.Zero || snapshot == NativeMethods.InvalidHandleValue)
            return result;

        try
        {
            var children = new Dictionary<int, List<int>>();
            var entry = new NativeMethods.PROCESSENTRY32 { dwSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.PROCESSENTRY32>() };
            if (NativeMethods.Process32First(snapshot, ref entry))
            {
                do
                {
                    var parent = (int)entry.th32ParentProcessID;
                    if (!children.TryGetValue(parent, out var list))
                        children[parent] = list = new List<int>();
                    list.Add((int)entry.th32ProcessID);
                } while (NativeMethods.Process32Next(snapshot, ref entry));
            }

            var queue = new Queue<int>(new[] { rootPid });
            while (queue.Count > 0)
            {
                var pid = queue.Dequeue();
                if (!children.TryGetValue(pid, out var kids)) continue;
                foreach (var kid in kids)
                    if (result.Add(kid))
                        queue.Enqueue(kid);
            }
        }
        finally
        {
            NativeMethods.CloseHandle(snapshot);
        }

        return result;
    }
}
