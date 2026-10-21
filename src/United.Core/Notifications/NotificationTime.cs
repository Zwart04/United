namespace United.Core.Notifications;

/// <summary>
/// Normalizes WebView2 notification timestamps. WebView2 exposes a UTC wall clock
/// with <see cref="DateTimeKind.Unspecified"/>; converting it directly to a
/// DateTimeOffset interprets it as local time and moves it back by the UTC offset.
/// The interpretation closest to "now" is the safe one for a live notification.
/// </summary>
public static class NotificationTime
{
    public static DateTimeOffset Normalize(DateTime value, DateTimeOffset now)
    {
        if (value == default) return now;
        if (value.Kind == DateTimeKind.Utc) return new DateTimeOffset(value);
        if (value.Kind == DateTimeKind.Local) return new DateTimeOffset(value);

        var asUtc = new DateTimeOffset(DateTime.SpecifyKind(value, DateTimeKind.Utc));
        var asLocal = new DateTimeOffset(DateTime.SpecifyKind(value, DateTimeKind.Local));
        return Math.Abs((asUtc - now).Ticks) <= Math.Abs((asLocal - now).Ticks)
            ? asUtc
            : asLocal;
    }
}
