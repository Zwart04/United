using United.Core.Notifications;
using Xunit;

namespace United.Core.Tests;

public class NotificationTimeTests
{
    private static readonly TimeSpan Jakarta = TimeSpan.FromHours(7);
    private static readonly DateTimeOffset Now =
        new(2026, 10, 21, 10, 50, 7, Jakarta);

    [Fact]
    public void Default_ReturnsNow()
    {
        Assert.Equal(Now, NotificationTime.Normalize(default, Now));
    }

    [Fact]
    public void UtcKind_PreservesInstant()
    {
        var value = new DateTime(2026, 10, 21, 3, 50, 7, DateTimeKind.Utc);

        var at = NotificationTime.Normalize(value, Now);

        Assert.Equal(TimeSpan.Zero, at.Offset);
        Assert.Equal(Now, at);
    }

    [Fact]
    public void LocalKind_PreservesInstant()
    {
        var value = new DateTime(2026, 10, 21, 10, 50, 7, DateTimeKind.Local);

        var at = NotificationTime.Normalize(value, Now);

        Assert.Equal(Now, at);
    }

    [Fact]
    public void UnspecifiedUtcClock_IsReadAsUtc()
    {
        // WebView2's CoreWebView2Notification.Timestamp is a UTC wall clock with
        // Kind=Unspecified. At 10:50 WIB it reads 03:50, not 10:50.
        var value = new DateTime(2026, 10, 21, 3, 50, 7, DateTimeKind.Unspecified);

        var at = NotificationTime.Normalize(value, Now);

        Assert.Equal(TimeSpan.Zero, at.Offset);
        Assert.Equal(Now, at);
    }

    [Fact]
    public void UnspecifiedLocalClock_IsReadAsLocal()
    {
        var value = new DateTime(2026, 10, 21, 10, 50, 7, DateTimeKind.Unspecified);

        var at = NotificationTime.Normalize(value, Now);

        Assert.Equal(Jakarta, at.Offset);
        Assert.Equal(Now, at);
    }
}
