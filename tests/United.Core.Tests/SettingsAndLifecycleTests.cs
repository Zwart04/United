using United.Core.Lifecycle;
using United.Core.Services;
using United.Core.Settings;
using Xunit;

namespace United.Core.Tests;

public class SettingsStoreTests
{
    private static string TempFile() =>
        Path.Combine(Path.GetTempPath(), "united-tests", Guid.NewGuid().ToString("N"), "settings.json");

    [Fact]
    public void MissingFile_ReturnsDefaults()
    {
        var store = new SettingsStore(TempFile());
        var s = store.Load();
        Assert.Equal("system", s.Theme);
        Assert.True(s.CloseMinimizesToTray);
        Assert.Contains("whatsapp", s.EnabledServices);
    }

    [Fact]
    public void RoundTrip_PreservesValues()
    {
        var path = TempFile();
        var store = new SettingsStore(path);
        var s = store.Load();
        s.Theme = "dark";
        s.LaunchAtStartup = true;
        s.EnabledServices = new List<string> { "telegram", "threads" };
        store.Save(s);

        var loaded = new SettingsStore(path).Load();
        Assert.Equal("dark", loaded.Theme);
        Assert.True(loaded.LaunchAtStartup);
        Assert.Equal(new[] { "telegram", "threads" }, loaded.EnabledServices);
    }

    [Fact]
    public void CorruptFile_QuarantinedAndDefaultsReturned()
    {
        var path = TempFile();
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, "{ not valid json !!");
        var store = new SettingsStore(path);
        var s = store.Load();
        Assert.Equal("system", s.Theme);
        Assert.True(File.Exists(path + ".bad"));
        Assert.False(File.Exists(path));
    }

    [Fact]
    public void Load_FromVersionlessFile_AddsFacebookOnce()
    {
        var path = TempFile();
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, """
        {
          "theme": "dark",
          "enabledServices": ["whatsapp", "instagram", "telegram", "discord", "threads"]
        }
        """);

        var loaded = new SettingsStore(path).Load();
        Assert.Equal(1, loaded.SettingsVersion);
        Assert.Equal(
            new[] { "whatsapp", "instagram", "facebook", "telegram", "discord", "threads" },
            loaded.EnabledServices);

        // A second load/save cycle must stay idempotent.
        new SettingsStore(path).Save(loaded);
        var again = new SettingsStore(path).Load();
        Assert.Equal(1, again.SettingsVersion);
        Assert.Single(again.EnabledServices, id => id == "facebook");
    }

    [Fact]
    public void Load_WithFacebookAlreadyPresent_DoesNotDuplicate()
    {
        var path = TempFile();
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, """
        {
          "enabledServices": ["facebook", "whatsapp"]
        }
        """);

        var loaded = new SettingsStore(path).Load();
        Assert.Equal(new[] { "facebook", "whatsapp" }, loaded.EnabledServices);
    }
}

public class ServiceCatalogTests
{
    [Fact]
    public void Resolve_KeepsEnabledOrder()
    {
        var list = ServiceCatalog.Resolve(new[] { "telegram", "whatsapp" }, null, out var problems);
        Assert.Equal(2, list.Count);
        Assert.Equal("telegram", list[0].Id);
        Assert.Empty(problems);
    }

    [Fact]
    public void Resolve_ReportsUnknownIds()
    {
        var list = ServiceCatalog.Resolve(new[] { "whatsapp", "nosuch" }, null, out var problems);
        Assert.Single(list);
        Assert.Contains(problems, p => p.Contains("nosuch"));
    }

    [Fact]
    public void Resolve_ParsesUserJsonAndRejectsInvalid()
    {
        var json = """
        [
          { "id": "mychat", "displayName": "My Chat", "url": "https://chat.example.com", "enabledByDefault": true },
          { "id": "broken", "displayName": "Broken", "url": "not-a-url" }
        ]
        """;
        var list = ServiceCatalog.Resolve(new[] { "whatsapp" }, json, out var problems);
        Assert.Contains(list, s => s.Id == "mychat");
        Assert.Contains(problems, p => p.Contains("broken"));
    }

    [Fact]
    public void BuiltInDefinitions_AreValid()
    {
        foreach (var s in ServiceCatalog.AllBuiltIn)
            Assert.True(s.IsValid(), $"{s.Id} must be valid");
    }

    [Fact]
    public void BuiltIn_IncludesFacebook()
    {
        var facebook = ServiceCatalog.Find("facebook");
        Assert.NotNull(facebook);
        Assert.Equal("Facebook", facebook!.DisplayName);
        Assert.Equal("facebook", facebook.Icon);
        Assert.StartsWith("https://www.facebook.com/", facebook.Url);
    }
}

public class LifecycleTests
{
    [Theory]
    [InlineData(ServiceState.NotCreated, ServiceState.Starting, true)]
    [InlineData(ServiceState.Starting, ServiceState.Ready, true)]
    [InlineData(ServiceState.Ready, ServiceState.Suspended, true)]
    [InlineData(ServiceState.Suspended, ServiceState.Ready, true)]
    [InlineData(ServiceState.Ready, ServiceState.Hibernated, true)]
    [InlineData(ServiceState.Hibernated, ServiceState.Starting, true)]
    [InlineData(ServiceState.Disposed, ServiceState.Starting, false)]
    [InlineData(ServiceState.NotCreated, ServiceState.Ready, false)]
    public void Transitions(ServiceState from, ServiceState to, bool expected)
        => Assert.Equal(expected, ServiceStateMachine.CanTransition(from, to));

    [Fact]
    public void Policy_LowersInactiveAndKeepsActive()
    {
        var policy = new MemoryGovernorPolicy(autoLowMemory: true);
        Assert.Equal(MemoryAction.None, policy.Decide(ServiceState.Ready, "whatsapp", "whatsapp", windowVisible: true, userHibernated: false));
        Assert.Equal(MemoryAction.Lower, policy.Decide(ServiceState.Ready, "telegram", "whatsapp", windowVisible: true, userHibernated: false));
        Assert.Equal(MemoryAction.Lower, policy.Decide(ServiceState.Ready, "whatsapp", "whatsapp", windowVisible: false, userHibernated: false));
    }

    [Fact]
    public void Policy_HonoursUserHibernation()
    {
        var policy = new MemoryGovernorPolicy(autoLowMemory: false);
        Assert.Equal(MemoryAction.Hibernate, policy.Decide(ServiceState.Ready, "discord", "discord", true, userHibernated: true));
        Assert.Equal(MemoryAction.None, policy.Decide(ServiceState.Ready, "discord", "discord", true, userHibernated: false));
    }

    [Fact]
    public void Policy_IgnoresAlreadyHibernated()
    {
        var policy = new MemoryGovernorPolicy(autoLowMemory: true);
        Assert.Equal(MemoryAction.None, policy.Decide(ServiceState.Hibernated, "x", "x", true, userHibernated: true));
    }
}
