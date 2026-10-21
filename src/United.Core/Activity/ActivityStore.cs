using System.Text.Json;
using System.Text.Json.Serialization;

namespace United.Core.Activity;

/// <summary>One conversation row in the unified inbox, keyed by service + title.</summary>
public sealed class ActivityEntry
{
    public string ServiceId { get; set; } = "";
    public string ServiceName { get; set; } = "";
    public string IconKey { get; set; } = "globe";
    public string Title { get; set; } = "";
    public string Body { get; set; } = "";
    /// <summary>Stable relative href of the conversation row (Discord /channels/@me/…).</summary>
    public string Target { get; set; } = "";
    public DateTimeOffset LastAt { get; set; }
    public int Count { get; set; }
}

/// <summary>
/// Persistent, bounded log of new-message notifications and explicitly supported
/// live chat previews. No tokens or message-sending automation. Messages that arrive
/// while a service is hibernated are not seen, and chat history is never imported.
/// </summary>
public sealed class ActivityStore
{
    public const int MaxEntries = 300;
    private const int MaxBodyLength = 400;
    private const int MaxTitleLength = 160;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    private readonly string _path;
    private readonly object _gate = new();
    private readonly object _saveGate = new();
    private readonly List<ActivityEntry> _entries = new();

    public ActivityStore(string path)
    {
        _path = path;
        Load();
    }

    public static string DefaultPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "United", "activity.json");

    public event Action? Changed;

    /// <summary>Newest first. The returned list is a copy and safe to enumerate.</summary>
    public IReadOnlyList<ActivityEntry> Snapshot()
    {
        lock (_gate) return _entries.OrderByDescending(e => e.LastAt).ToList();
    }

    public int UnreadTotal
    {
        get
        {
            lock (_gate)
            {
                var total = 0;
                foreach (var e in _entries) total += Math.Max(0, e.Count);
                return total;
            }
        }
    }

    /// <summary>
    /// Records a notification. Repeated notifications for the same service+title
    /// update the existing row (latest body/time) and increment its unread count.
    /// </summary>
    public void Add(string serviceId, string serviceName, string iconKey, string title, string body,
        DateTimeOffset at, string? target = null, int? unread = null)
    {
        if (string.IsNullOrWhiteSpace(serviceId)) return;

        var safeTitle = Clean(title, "New message", MaxTitleLength);
        var safeBody = Clean(body, "", MaxBodyLength);
        var safeTarget = (target ?? "").Trim();
        if (safeTarget.Length > 600) safeTarget = safeTarget[..600];
        if (at == default) at = DateTimeOffset.Now;

        lock (_gate)
        {
            ActivityEntry? entry = null;
            if (safeTarget.Length > 0)
            {
                entry = _entries.FirstOrDefault(e =>
                    string.Equals(e.ServiceId, serviceId, StringComparison.OrdinalIgnoreCase) &&
                    string.Equals(e.Target, safeTarget, StringComparison.OrdinalIgnoreCase));
                if (entry == null)
                {
                    // Adopt a notification row for the same conversation that had no href.
                    entry = _entries.FirstOrDefault(e =>
                        string.Equals(e.ServiceId, serviceId, StringComparison.OrdinalIgnoreCase) &&
                        string.IsNullOrWhiteSpace(e.Target) &&
                        string.Equals(e.Title, safeTitle, StringComparison.OrdinalIgnoreCase));
                }
                if (entry != null)
                {
                    // Older builds keyed conversations by the volatile title; merge those
                    // duplicates into the row that now has a stable href.
                    foreach (var duplicate in _entries.Where(e => !ReferenceEquals(e, entry) &&
                        string.Equals(e.ServiceId, serviceId, StringComparison.OrdinalIgnoreCase) &&
                        string.Equals(e.Target, safeTarget, StringComparison.OrdinalIgnoreCase)).ToList())
                    {
                        if (duplicate.Count > entry.Count) entry.Count = duplicate.Count;
                        _entries.Remove(duplicate);
                    }
                }
            }
            else
            {
                entry = _entries.FirstOrDefault(e =>
                    string.Equals(e.ServiceId, serviceId, StringComparison.OrdinalIgnoreCase) &&
                    string.Equals(e.Title, safeTitle, StringComparison.OrdinalIgnoreCase));
            }

            if (entry == null)
            {
                entry = new ActivityEntry { ServiceId = serviceId, Title = safeTitle };
                _entries.Add(entry);
            }

            entry.Title = safeTitle;
            entry.ServiceName = string.IsNullOrWhiteSpace(serviceName) ? serviceId : serviceName;
            entry.IconKey = string.IsNullOrWhiteSpace(iconKey) ? "globe" : iconKey;
            entry.Body = safeBody;
            if (safeTarget.Length > 0) entry.Target = safeTarget;
            entry.LastAt = at;
            // The observer sends the row's real unread state; notifications (no value)
            // keep the historical increment behaviour.
            entry.Count = unread.HasValue
                ? Math.Clamp(unread.Value, 0, 999)
                : Math.Min(999, entry.Count + 1);

            _entries.Sort((a, b) => b.LastAt.CompareTo(a.LastAt));
            if (_entries.Count > MaxEntries)
                _entries.RemoveRange(MaxEntries, _entries.Count - MaxEntries);
        }

        Save();
        Changed?.Invoke();
    }

    public void MarkRead(string serviceId, string title)
    {
        var changed = false;
        lock (_gate)
        {
            foreach (var e in _entries)
            {
                if (!string.Equals(e.ServiceId, serviceId, StringComparison.OrdinalIgnoreCase)) continue;
                if (!string.Equals(e.Title, title, StringComparison.OrdinalIgnoreCase)) continue;
                if (e.Count == 0) continue;
                e.Count = 0;
                changed = true;
            }
        }

        if (!changed) return;
        Save();
        Changed?.Invoke();
    }

    public void MarkAllRead()
    {
        var changed = false;
        lock (_gate)
        {
            foreach (var e in _entries)
            {
                if (e.Count == 0) continue;
                e.Count = 0;
                changed = true;
            }
        }

        if (!changed) return;
        Save();
        Changed?.Invoke();
    }

    public void Clear()
    {
        int removed;
        lock (_gate)
        {
            removed = _entries.Count;
            _entries.Clear();
        }

        if (removed == 0) return;
        Save();
        Changed?.Invoke();
    }

    private void Load()
    {
        try
        {
            if (!File.Exists(_path)) return;
            var json = File.ReadAllText(_path);
            var loaded = JsonSerializer.Deserialize<List<ActivityEntry>>(json, JsonOptions);
            if (loaded == null) return;

            lock (_gate)
            {
                _entries.Clear();
                var changed = false;
                // Newest first, so the surviving duplicate keeps the latest preview.
                foreach (var e in loaded.OrderByDescending(x => x.LastAt))
                {
                    if (string.IsNullOrWhiteSpace(e.ServiceId) || string.IsNullOrWhiteSpace(e.Title)) continue;
                    e.Count = Math.Clamp(e.Count, 0, 999);
                    var existing = _entries.FirstOrDefault(x => SameConversation(x, e));
                    if (existing != null)
                    {
                        existing.Count = Math.Max(existing.Count, e.Count);
                        changed = true;
                        continue;
                    }
                    _entries.Add(e);
                }

                _entries.Sort((a, b) => b.LastAt.CompareTo(a.LastAt));
                if (_entries.Count > MaxEntries)
                {
                    _entries.RemoveRange(MaxEntries, _entries.Count - MaxEntries);
                    changed = true;
                }

                if (changed) Save();
            }
        }
        catch
        {
            // A corrupt or unreadable activity file must never block startup.
        }
    }

    private void Save()
    {
        List<ActivityEntry> snapshot;
        lock (_gate) snapshot = _entries.ToList();

        lock (_saveGate)
        {
            try
            {
                var dir = Path.GetDirectoryName(_path);
                if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
                var json = JsonSerializer.Serialize(snapshot, JsonOptions);
                var tmp = _path + ".tmp";
                File.WriteAllText(tmp, json);
                File.Move(tmp, _path, overwrite: true);
            }
            catch
            {
                // Activity history is a convenience; never crash the app over it.
            }
        }
    }

    private static bool SameConversation(ActivityEntry a, ActivityEntry b)
    {
        if (!string.Equals(a.ServiceId, b.ServiceId, StringComparison.OrdinalIgnoreCase))
            return false;
        if (!string.IsNullOrWhiteSpace(a.Target) && !string.IsNullOrWhiteSpace(b.Target))
            return string.Equals(a.Target, b.Target, StringComparison.OrdinalIgnoreCase);
        return string.Equals(a.Title, b.Title, StringComparison.OrdinalIgnoreCase);
    }

    private static string Clean(string? value, string fallback, int max)
    {
        if (string.IsNullOrWhiteSpace(value)) return fallback;
        var text = value.Replace('\r', ' ').Replace('\n', ' ').Trim();
        return text.Length <= max ? text : text[..max];
    }
}
