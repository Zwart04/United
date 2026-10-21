namespace United.Core.ReadState;

/// <summary>
/// Mutable per-service badge state. Returns change descriptors so the UI layer
/// only reacts when something actually changed (no unnecessary re-renders).
/// </summary>
public sealed class BadgeState
{
    private readonly object _gate = new();
    private readonly Dictionary<string, int> _counts = new(StringComparer.OrdinalIgnoreCase);

    public event Action? Changed;

    public void Set(string serviceId, BadgeReading reading)
    {
        bool changed;
        lock (_gate)
        {
            // "No signal" resets to zero; "Dot" is stored as -1; exact counts as-is.
            var value = !reading.Matched ? 0 : reading.Count;
            _counts.TryGetValue(serviceId, out var previous);
            changed = previous != value;
            if (changed) _counts[serviceId] = value;
        }
        if (changed) Changed?.Invoke();
    }

    /// <summary>Sets an exact count discovered outside document.title (e.g. in-page unread badges).</summary>
    public void SetExact(string serviceId, int count)
    {
        if (string.IsNullOrWhiteSpace(serviceId)) return;
        var value = count < 0 ? -1 : count > 999_999 ? 999_999 : count;

        bool changed;
        lock (_gate)
        {
            _counts.TryGetValue(serviceId, out var previous);
            changed = previous != value;
            if (changed) _counts[serviceId] = value;
        }
        if (changed) Changed?.Invoke();
    }

    public void Clear(string serviceId)
    {
        bool changed;
        lock (_gate) changed = _counts.Remove(serviceId);
        if (changed) Changed?.Invoke();
    }

    public int Get(string serviceId)
    {
        lock (_gate) return _counts.TryGetValue(serviceId, out var v) ? v : 0;
    }

    /// <summary>Sum of positive counts; dot (-1) entries count as 1 so the tray never shows 0 while unread exists.</summary>
    public int Total
    {
        get
        {
            lock (_gate)
            {
                var total = 0;
                foreach (var v in _counts.Values) total += v < 0 ? 1 : v;
                return total;
            }
        }
    }
}
