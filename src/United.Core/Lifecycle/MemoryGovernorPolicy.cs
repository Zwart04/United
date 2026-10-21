using United.Core.Services;

namespace United.Core.Lifecycle;

/// <summary>
/// Decides memory policy for a service. Pure decision logic (unit tested);
/// the WPF layer applies the resulting instruction to CoreWebView2.
/// </summary>
public enum MemoryAction
{
    /// <summary>Keep MemoryUsageTargetLevel at Normal.</summary>
    None,

    /// <summary>Set MemoryUsageTargetLevel = Low (scripts and network connections keep running).</summary>
    Lower,

    /// <summary>Dispose the controller; profile data is preserved (manual hibernation).</summary>
    Hibernate,
}

public sealed class MemoryGovernorPolicy
{
    private readonly bool _autoLowMemory;

    public MemoryGovernorPolicy(bool autoLowMemory)
    {
        _autoLowMemory = autoLowMemory;
    }

    /// <summary>
    /// What should happen to <paramref name="state"/> when <paramref name="activeServiceId"/>
    /// has focus and the window visibility is <paramref name="windowVisible"/>.
    /// </summary>
    public MemoryAction Decide(ServiceState state, string serviceId, string activeServiceId, bool windowVisible, bool userHibernated)
    {
        if (userHibernated && state is ServiceState.Ready or ServiceState.Suspended)
            return MemoryAction.Hibernate;

        if (state is ServiceState.Hibernated or ServiceState.NotCreated or ServiceState.Disposed)
            return MemoryAction.None;

        // Hibernated services that the user resumed need recreation, handled by lifecycle layer.

        var isActive = string.Equals(serviceId, activeServiceId, StringComparison.OrdinalIgnoreCase);
        if (isActive && windowVisible)
            return MemoryAction.None; // foreground: full speed

        if (!_autoLowMemory)
            return MemoryAction.None;

        return MemoryAction.Lower;
    }
}
