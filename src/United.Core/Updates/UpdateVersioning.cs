using System.Text.RegularExpressions;

namespace United.Core.Updates;

/// <summary>
/// Version and checksum helpers for the GitHub release updater. Kept free of
/// network/UI code so the comparison rules can be unit tested.
/// </summary>
public static class UpdateVersioning
{
    private static readonly Regex TagVersion = new(
        @"(?<!\d)(\d+)\.(\d+)(?:\.(\d+))?",
        RegexOptions.CultureInvariant | RegexOptions.Compiled);

    private static readonly Regex Sha256 = new(
        @"(?<![0-9a-fA-F])([0-9a-fA-F]{64})(?![0-9a-fA-F])",
        RegexOptions.CultureInvariant | RegexOptions.Compiled);

    /// <summary>Version without the revision component, which is always 0 for releases.</summary>
    public static Version Normalize(Version version)
        => new(version.Major, version.Minor, Math.Max(version.Build, 0));

    /// <summary>Parses a release tag such as <c>v0.2.2</c> or <c>0.2.2</c>.</summary>
    public static bool TryParseTag(string? tag, out Version version)
    {
        version = new Version(0, 0, 0);
        if (string.IsNullOrWhiteSpace(tag)) return false;

        var match = TagVersion.Match(tag);
        if (!match.Success) return false;

        var major = int.Parse(match.Groups[1].Value);
        var minor = int.Parse(match.Groups[2].Value);
        var build = match.Groups[3].Success ? int.Parse(match.Groups[3].Value) : 0;
        version = new Version(major, minor, build);
        return true;
    }

    /// <summary>True when <paramref name="candidate"/> is a newer release than <paramref name="current"/>.</summary>
    public static bool IsNewer(Version candidate, Version current)
        => Normalize(candidate) > Normalize(current);

    /// <summary>
    /// Extracts the first SHA-256 digest from a <c>sha256sum</c>-style file.
    /// Returns lowercase hex, or null when the file has no digest.
    /// </summary>
    public static string? ParseSha256(string? content)
    {
        if (string.IsNullOrWhiteSpace(content)) return null;
        var match = Sha256.Match(content);
        return match.Success ? match.Groups[1].Value.ToLowerInvariant() : null;
    }
}
