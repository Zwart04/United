using System.Text.RegularExpressions;
using United.Core.Services;

namespace United.Core.ReadState;

/// <summary>Result of parsing a document title for unread information.</summary>
public readonly record struct BadgeReading(bool Matched, int Count)
{
    /// <summary>No pattern matched — treated as "no unread signal" (count 0).</summary>
    public static readonly BadgeReading None = new(false, 0);

    /// <summary>Matched but without a number: unread exists, exact count unknown.</summary>
    public static readonly BadgeReading Dot = new(true, -1);

    public static BadgeReading Exact(int count) => new(true, count);
}

/// <summary>
/// Derives per-service unread state from document.title.
/// Title-based detection is the primary, least fragile mechanism: every major service
/// exports "(n) Service" style titles for exactly this purpose.
/// </summary>
public static class UnreadParser
{
    private static readonly TimeSpan RegexTimeout = TimeSpan.FromMilliseconds(50);

    public static BadgeReading Parse(ServiceDefinition? service, string? title)
    {
        if (service == null || string.IsNullOrWhiteSpace(title)) return BadgeReading.None;

        foreach (var pattern in service.TitlePatterns)
        {
            if (string.IsNullOrWhiteSpace(pattern)) continue;
            Match match;
            try
            {
                match = Regex.Match(title, pattern, RegexOptions.None, RegexTimeout);
            }
            catch (RegexMatchTimeoutException)
            {
                continue; // a bad user pattern must never crash the app
            }
            catch (ArgumentException)
            {
                continue;
            }

            if (!match.Success) continue;

            var countGroup = match.Groups["count"];
            if (countGroup.Success &&
                int.TryParse(countGroup.Value, out var count) &&
                count >= 0 && count <= 999_999)
            {
                return BadgeReading.Exact(count);
            }

            return BadgeReading.Dot;
        }

        return BadgeReading.None;
    }
}
