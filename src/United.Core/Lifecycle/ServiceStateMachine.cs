namespace United.Core.Lifecycle;

/// <summary>
/// Lifecycle of one hosted service webview.
///
/// NotCreated  -> Starting   (controller being created)
/// Starting    -> Ready      (first navigation completed or failed event observed)
/// Starting    -> Failed
/// Ready       -> Suspended  (MemoryUsageTargetLevel = Low; scripts + sockets keep running)
/// Suspended   -> Ready      (focused again -> Normal)
/// Ready/Susp. -> Hibernated (controller disposed, profile kept)
/// Hibernated  -> Starting   (user re-selects the service)
/// Failed      -> Starting   (retry)
/// Ready/Susp./Failed/Hibernated -> Disposed (app shutdown or service removed)
/// </summary>
public enum ServiceState
{
    NotCreated,
    Starting,
    Ready,
    Suspended,
    Hibernated,
    Failed,
    Disposed,
}

public static class ServiceStateMachine
{
    private static readonly Dictionary<ServiceState, ServiceState[]> Allowed = new()
    {
        [ServiceState.NotCreated] = new[] { ServiceState.Starting, ServiceState.Disposed },
        [ServiceState.Starting] = new[] { ServiceState.Ready, ServiceState.Failed, ServiceState.Disposed },
        [ServiceState.Ready] = new[] { ServiceState.Suspended, ServiceState.Hibernated, ServiceState.Failed, ServiceState.Disposed },
        [ServiceState.Suspended] = new[] { ServiceState.Ready, ServiceState.Hibernated, ServiceState.Failed, ServiceState.Disposed },
        [ServiceState.Hibernated] = new[] { ServiceState.Starting, ServiceState.Disposed },
        [ServiceState.Failed] = new[] { ServiceState.Starting, ServiceState.Disposed },
        [ServiceState.Disposed] = Array.Empty<ServiceState>(),
    };

    public static bool CanTransition(ServiceState from, ServiceState to)
    {
        if (from == to) return true;
        return Allowed.TryGetValue(from, out var next) && Array.IndexOf(next, to) >= 0;
    }

    /// <summary>True when the webview exists and scripts/network connections must stay alive.</summary>
    public static bool IsLive(ServiceState s) => s is ServiceState.Ready or ServiceState.Suspended;

    /// <summary>True when the state requires MemoryUsageTargetLevel == Low.</summary>
    public static bool WantsLowMemory(ServiceState s) => s == ServiceState.Suspended;
}
