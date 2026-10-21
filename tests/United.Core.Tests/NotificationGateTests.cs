using United.Core.Notifications;
using Xunit;

namespace United.Core.Tests;

public class NotificationGateTests
{
    private static readonly DateTimeOffset T0 = new(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);

    [Fact]
    public void FirstNotification_AlwaysPasses()
    {
        var gate = new NotificationGate();
        Assert.True(gate.ShouldNotify(NotificationGate.KeyFor("a", "b"), T0));
    }

    [Fact]
    public void MinimumInterval_Throttles()
    {
        var gate = new NotificationGate(minInterval: TimeSpan.FromMilliseconds(1500));
        Assert.True(gate.ShouldNotify("k1", T0));
        Assert.False(gate.ShouldNotify("k2", T0 + TimeSpan.FromMilliseconds(500)));
        Assert.True(gate.ShouldNotify("k2", T0 + TimeSpan.FromMilliseconds(1600)));
    }

    [Fact]
    public void ExactDuplicate_IsSuppressedInsideWindow()
    {
        var gate = new NotificationGate(minInterval: TimeSpan.Zero);
        var key = NotificationGate.KeyFor("WhatsApp", "hello");
        Assert.True(gate.ShouldNotify(key, T0));
        Assert.False(gate.ShouldNotify(key, T0 + TimeSpan.FromSeconds(5)));
        Assert.True(gate.ShouldNotify(key, T0 + TimeSpan.FromSeconds(61)));
    }

    [Fact]
    public void DifferentMessages_DoNotSuppressEachOther()
    {
        var gate = new NotificationGate(minInterval: TimeSpan.Zero);
        Assert.True(gate.ShouldNotify(NotificationGate.KeyFor("t", "m1"), T0));
        Assert.True(gate.ShouldNotify(NotificationGate.KeyFor("t", "m2"), T0 + TimeSpan.FromSeconds(1)));
    }

    [Fact]
    public void StormCap_LimitsBurst()
    {
        var gate = new NotificationGate(minInterval: TimeSpan.Zero, stormLimit: 3);
        Assert.True(gate.ShouldNotify("a", T0));
        Assert.True(gate.ShouldNotify("b", T0 + TimeSpan.FromSeconds(1)));
        Assert.True(gate.ShouldNotify("c", T0 + TimeSpan.FromSeconds(2)));
        Assert.False(gate.ShouldNotify("d", T0 + TimeSpan.FromSeconds(3)));
        // window slides: old entries expire
        Assert.True(gate.ShouldNotify("e", T0 + TimeSpan.FromSeconds(14)));
    }
}
