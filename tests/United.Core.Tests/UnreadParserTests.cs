using United.Core.ReadState;
using United.Core.Services;
using Xunit;

namespace United.Core.Tests;

public class UnreadParserTests
{
    private static ServiceDefinition Def(params string[] patterns) => new()
    {
        Id = "test", DisplayName = "Test", Url = "https://example.com", TitlePatterns = patterns,
    };

    [Fact]
    public void ExactCount_IsExtracted()
    {
        var r = UnreadParser.Parse(Def(@"^\((?<count>\d+)\)"), "(3) WhatsApp");
        Assert.True(r.Matched);
        Assert.Equal(3, r.Count);
    }

    [Fact]
    public void DiscordChannelTitle_IsExtracted()
    {
        var r = UnreadParser.Parse(Def(@"^\((?<count>\d+)\)"), "(12) Discord | #general");
        Assert.True(r.Matched);
        Assert.Equal(12, r.Count);
    }

    [Fact]
    public void DiscordTitleMentions_AreNotTreatedAsDmBadge()
    {
        // Discord's "(n)" counts server mentions; the DM badge comes from the
        // Direct Messages icon through the live observer.
        var discord = ServiceCatalog.Find("discord");
        Assert.NotNull(discord);
        var r = UnreadParser.Parse(discord, "(29761) Discord | @TestBot");
        Assert.False(r.Matched);
    }

    [Fact]
    public void MessengerCappedTitle_IsExtracted()
    {
        var r = UnreadParser.Parse(Def(@"^\((?<count>\d+)\+?\)"), "(20+) Messenger | Facebook");
        Assert.True(r.Matched);
        Assert.Equal(20, r.Count);
    }

    [Fact]
    public void ThreadsMessagesTitle_IsExtracted()
    {
        var r = UnreadParser.Parse(Def(@"^\((?<count>\d+)\)"), "(1) Messages • Threads");
        Assert.True(r.Matched);
        Assert.Equal(1, r.Count);
    }

    [Fact]
    public void NoMatchMeansNoUnreadSignal()
    {
        var r = UnreadParser.Parse(Def(@"^\((?<count>\d+)\)"), "WhatsApp");
        Assert.False(r.Matched);
    }

    [Fact]
    public void PatternWithoutCountGroup_YieldsDot()
    {
        var r = UnreadParser.Parse(Def(@"^•"), "• Telegram");
        Assert.True(r.Matched);
        Assert.Equal(-1, r.Count);
    }

    [Fact]
    public void EmptyTitleAndNullService_AreSafe()
    {
        Assert.False(UnreadParser.Parse(Def(@"^\((?<count>\d+)\)"), "").Matched);
        Assert.False(UnreadParser.Parse(null, "(5) X").Matched);
    }

    [Fact]
    public void InvalidUserRegex_NeverThrows()
    {
        var r = UnreadParser.Parse(Def("(unclosed"), "(2) WhatsApp");
        Assert.False(r.Matched);
    }

    [Fact]
    public void MultiplePatterns_AreTriedInOrder()
    {
        var def = Def(@"^•", @"^\((?<count>\d+)\)");
        Assert.Equal(7, UnreadParser.Parse(def, "(7) Telegram").Count);
    }

    [Fact]
    public void AbsurdCount_IsTreatedAsDot()
    {
        var r = UnreadParser.Parse(Def(@"^\((?<count>\d+)\)"), "(10000000) X");
        Assert.True(r.Matched);
        Assert.Equal(-1, r.Count);
    }
}

public class BadgeStateTests
{
    [Fact]
    public void Set_RaisesChange_OnlyWhenValueChanges()
    {
        var state = new BadgeState();
        var changes = 0;
        state.Changed += () => changes++;

        state.Set("wa", BadgeReading.Exact(3));
        state.Set("wa", BadgeReading.Exact(3));
        state.Set("wa", BadgeReading.None);

        Assert.Equal(2, changes);
        Assert.Equal(0, state.Get("wa"));
    }

    [Fact]
    public void DotCountsAsOneInTotal()
    {
        var state = new BadgeState();
        state.Set("wa", BadgeReading.Exact(2));
        state.Set("ig", BadgeReading.Dot);
        state.Set("tg", BadgeReading.None);
        Assert.Equal(3, state.Total);
    }

    [Fact]
    public void Clear_RemovesAndRaises()
    {
        var state = new BadgeState();
        state.Set("wa", BadgeReading.Exact(1));
        var changes = 0;
        state.Changed += () => changes++;
        state.Clear("wa");
        state.Clear("wa");
        Assert.Equal(1, changes);
        Assert.Equal(0, state.Total);
    }
}
