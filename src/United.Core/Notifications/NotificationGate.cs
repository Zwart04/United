namespace United.Core.Notifications;

/// <summary>
/// Per-service gate that prevents notification storms and duplicate toasts.
/// Pure logic, clock injected — unit tested.
/// </summary>
public sealed class NotificationGate
{
    private readonly object _gate = new();
    private readonly TimeSpan _minInterval;
    private readonly TimeSpan _duplicateWindow;
    private readonly TimeSpan _stormWindow;
    private readonly int _stormLimit;

    private string? _lastKey;
    private DateTimeOffset _lastAt;
    private readonly Queue<DateTimeOffset> _recent = new();

    public NotificationGate(
        TimeSpan? minInterval = null,
        TimeSpan? duplicateWindow = null,
        TimeSpan? stormWindow = null,
        int stormLimit = 4)
    {
        _minInterval = minInterval ?? TimeSpan.FromMilliseconds(1500);
        _duplicateWindow = duplicateWindow ?? TimeSpan.FromSeconds(60);
        _stormWindow = stormWindow ?? TimeSpan.FromSeconds(10);
        _stormLimit = stormLimit;
    }

    /// <summary>
    /// Returns true when a notification with <paramref name="dedupeKey"/> should be shown now.
    /// Rules: per-service minimum interval, exact-duplicate suppression inside the window,
    /// and a rolling storm cap.
    /// </summary>
    public bool ShouldNotify(string dedupeKey, DateTimeOffset now)
    {
        lock (_gate)
        {
            if (_lastAt != default && now - _lastAt < _minInterval)
                return false;

            if (_lastKey == dedupeKey && now - _lastAt < _duplicateWindow)
                return false;

            while (_recent.Count > 0 && now - _recent.Peek() > _stormWindow)
                _recent.Dequeue();
            if (_recent.Count >= _stormLimit)
                return false;

            _recent.Enqueue(now);
            _lastAt = now;
            _lastKey = dedupeKey;
            return true;
        }
    }

    public static string KeyFor(string title, string body)
        => $"{title}\u001f{body}";
}
