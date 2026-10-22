using United.Core.Activity;
using Xunit;

namespace United.Core.Tests;

public sealed class ActivityStoreTests : IDisposable
{
    private readonly string _path = Path.Combine(
        Path.GetTempPath(), "united-activity-tests", Guid.NewGuid().ToString("N") + ".json");

    public void Dispose()
    {
        try { File.Delete(_path); } catch { /* best effort */ }
        try
        {
            var dir = Path.GetDirectoryName(_path);
            if (dir != null && Directory.Exists(dir) && !Directory.EnumerateFileSystemEntries(dir).Any())
                Directory.Delete(dir);
        }
        catch { /* best effort */ }
    }

    [Fact]
    public void Add_GroupsSameConversation_AndKeepsLatestBody()
    {
        var store = new ActivityStore(_path);
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);

        store.Add("whatsapp", "WhatsApp", "whatsapp", "Ibu", "first", t0);
        store.Add("whatsapp", "WhatsApp", "whatsapp", "Ibu", "second", t0.AddMinutes(1));

        var entries = store.Snapshot();
        var entry = Assert.Single(entries);
        Assert.Equal("second", entry.Body);
        Assert.Equal(2, entry.Count);
        Assert.Equal(t0.AddMinutes(1), entry.LastAt);
    }

    [Fact]
    public void Add_KeepsServicesSeparate_AndSortsNewestFirst()
    {
        var store = new ActivityStore(_path);
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);

        store.Add("whatsapp", "WhatsApp", "whatsapp", "Ibu", "hello", t0);
        store.Add("telegram", "Telegram", "telegram", "Ibu", "hi", t0.AddMinutes(2));

        var entries = store.Snapshot();
        Assert.Equal(2, entries.Count);
        Assert.Equal("telegram", entries[0].ServiceId);
        Assert.Equal("whatsapp", entries[1].ServiceId);
    }

    [Fact]
    public void MarkRead_AndMarkAllRead_ResetUnreadCounts()
    {
        var store = new ActivityStore(_path);
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);
        store.Add("whatsapp", "WhatsApp", "whatsapp", "Ibu", "a", t0);
        store.Add("whatsapp", "WhatsApp", "whatsapp", "Ibu", "b", t0.AddMinutes(1));
        store.Add("discord", "Discord", "discord", "Guild", "c", t0.AddMinutes(2));
        Assert.Equal(3, store.UnreadTotal);

        store.MarkRead("whatsapp", "Ibu");
        Assert.Equal(1, store.UnreadTotal);
        Assert.Equal(0, store.Snapshot().Single(e => e.ServiceId == "whatsapp").Count);

        store.MarkAllRead();
        Assert.Equal(0, store.UnreadTotal);
    }

    [Fact]
    public void UnreadFor_SumsOnlyTheRequestedService()
    {
        var store = new ActivityStore(_path);
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);
        store.Add("whatsapp", "WhatsApp", "whatsapp", "Ibu", "a", t0);
        store.Add("whatsapp", "WhatsApp", "whatsapp", "Bapak", "b", t0.AddMinutes(1));
        store.Add("telegram", "Telegram", "telegram", "Ibu", "c", t0.AddMinutes(2));

        Assert.Equal(2, store.UnreadFor("whatsapp"));
        Assert.Equal(1, store.UnreadFor("telegram"));
        Assert.Equal(0, store.UnreadFor("discord"));

        store.MarkRead("whatsapp", "Ibu");
        Assert.Equal(1, store.UnreadFor("whatsapp"));
    }

    [Fact]
    public void Add_StoresTargetAndUsesAuthoritativeUnread()
    {
        var store = new ActivityStore(_path);
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);
        const string target = "/channels/@me/123456789012345678";

        store.Add("discord", "Discord", "discord", "Test Bot", "first", t0, target, 0);
        var entry = Assert.Single(store.Snapshot());
        Assert.Equal(target, entry.Target);
        Assert.Equal(0, entry.Count);

        store.Add("discord", "Discord", "discord", "Test Bot", "second", t0.AddMinutes(1), null, 2);
        entry = Assert.Single(store.Snapshot());
        Assert.Equal("second", entry.Body);
        Assert.Equal(target, entry.Target); // kept when the new event has no href
        Assert.Equal(2, entry.Count);

        store.Add("discord", "Discord", "discord", "Test Bot", "notification", t0.AddMinutes(2));
        entry = Assert.Single(store.Snapshot());
        Assert.Equal(3, entry.Count); // no snapshot: notification increments
    }

    [Fact]
    public void Add_SameTarget_KeepsOneConversationWhenTitleChanges()
    {
        var store = new ActivityStore(_path);
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);

        store.Add("telegram", "Telegram", "telegram", "Old preview", "old", t0, "#123", 1);
        store.Add("telegram", "Telegram", "telegram", "New preview", "new", t0.AddMinutes(1), "#123", 1);

        var entry = Assert.Single(store.Snapshot());
        Assert.Equal("New preview", entry.Title);
        Assert.Equal("new", entry.Body);
        Assert.Equal("#123", entry.Target);
        Assert.Equal(1, entry.Count); // live unread snapshots do not accumulate
    }

    [Fact]
    public void Load_DeduplicatesEntriesForTheSameTarget()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_path)!);
        File.WriteAllText(_path, """
            [
              {"serviceId":"telegram","serviceName":"Telegram","iconKey":"telegram","title":"Old","body":"old","target":"#123","lastAt":"2026-10-20T10:00:00+00:00","count":2},
              {"serviceId":"telegram","serviceName":"Telegram","iconKey":"telegram","title":"New","body":"new","target":"#123","lastAt":"2026-10-20T10:01:00+00:00","count":1}
            ]
            """);

        var store = new ActivityStore(_path);
        var entry = Assert.Single(store.Snapshot());
        Assert.Equal("New", entry.Title);
        Assert.Equal("new", entry.Body);
        Assert.Equal(2, entry.Count); // max of the merged unread snapshots
    }

    [Fact]
    public void PersistsAndReloads()
    {
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);
        var first = new ActivityStore(_path);
        first.Add("facebook", "Facebook", "facebook", "Ayu", "halo", t0);

        var second = new ActivityStore(_path);
        var entry = Assert.Single(second.Snapshot());
        Assert.Equal("facebook", entry.ServiceId);
        Assert.Equal("Ayu", entry.Title);
        Assert.Equal("halo", entry.Body);
        Assert.Equal(1, entry.Count);
    }

    [Fact]
    public void IsBounded()
    {
        var store = new ActivityStore(_path);
        var t0 = new DateTimeOffset(2026, 10, 20, 10, 0, 0, TimeSpan.Zero);
        for (var i = 0; i < ActivityStore.MaxEntries + 20; i++)
            store.Add("whatsapp", "WhatsApp", "whatsapp", "person-" + i, "body", t0.AddSeconds(i));

        Assert.Equal(ActivityStore.MaxEntries, store.Snapshot().Count);
    }

    [Fact]
    public void CorruptFile_IsIgnored()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_path)!);
        File.WriteAllText(_path, "{not json");
        var store = new ActivityStore(_path);
        Assert.Empty(store.Snapshot());
    }
}
