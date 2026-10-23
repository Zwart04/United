using United.Core.Updates;
using Xunit;

namespace United.Core.Tests;

public class UpdateVersioningTests
{
    [Theory]
    [InlineData("v0.2.2", 0, 2, 2)]
    [InlineData("0.2.2", 0, 2, 2)]
    [InlineData("v1.0", 1, 0, 0)]
    [InlineData("release-3.4.5", 3, 4, 5)]
    public void TryParseTag_AcceptsCommonTags(string tag, int major, int minor, int build)
    {
        Assert.True(UpdateVersioning.TryParseTag(tag, out var version));
        Assert.Equal(new Version(major, minor, build), version);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("latest")]
    public void TryParseTag_RejectsMissingVersion(string? tag)
    {
        Assert.False(UpdateVersioning.TryParseTag(tag, out _));
    }

    [Fact]
    public void IsNewer_IgnoresAssemblyRevision()
    {
        Assert.True(UpdateVersioning.IsNewer(new Version(0, 2, 2), new Version(0, 2, 1, 0)));
        Assert.False(UpdateVersioning.IsNewer(new Version(0, 2, 1), new Version(0, 2, 1, 0)));
        Assert.False(UpdateVersioning.IsNewer(new Version(0, 2, 0), new Version(0, 2, 1)));
    }

    [Fact]
    public void ParseSha256_ReadsSha256SumFormat()
    {
        const string body =
            "A155DB4BA41247F441501F12C3D30EAF1A9739118FC1BC49C37D191342A37A9A  United-Setup-0.2.1.exe\n";
        Assert.Equal(
            "a155db4ba41247f441501f12c3d30eaf1a9739118fc1bc49c37d191342a37a9a",
            UpdateVersioning.ParseSha256(body));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("not a checksum")]
    [InlineData("abc123")]
    public void ParseSha256_RejectsInvalidContent(string? body)
    {
        Assert.Null(UpdateVersioning.ParseSha256(body));
    }
}
