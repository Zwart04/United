using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using United.App.Diagnostics;
using United.Core.Updates;

namespace United.App.Services;

public sealed record UpdateRelease(
    Version Version,
    string TagName,
    string ReleaseUrl,
    string InstallerUrl,
    string Sha256Url,
    long InstallerSize);

public sealed record UpdateCheckResult(bool UpdateAvailable, Version? Version, string? Error)
{
    public static UpdateCheckResult UpToDate(Version version) => new(false, version, null);
    public static UpdateCheckResult Failed(string error) => new(false, null, error);
}

/// <summary>
/// Checks GitHub Releases, downloads the signed installer asset, verifies its
/// published SHA-256, and hands off to a small PowerShell helper so the app can
/// close before Inno Setup replaces its files.
/// </summary>
public sealed class UpdateService
{
    private const string Owner = "Zwart04";
    private const string Repo = "United";
    private const string ApiLatest = $"https://api.github.com/repos/{Owner}/{Repo}/releases/latest";
    private const string ReleasesPage = $"https://github.com/{Owner}/{Repo}/releases/latest";

    private static readonly TimeSpan CacheWindow = TimeSpan.FromMinutes(10);
    private static readonly HttpClient Http = CreateHttpClient();

    private DateTimeOffset _lastCheckUtc = DateTimeOffset.MinValue;
    private UpdateCheckResult? _lastResult;

    public static UpdateService Current { get; } = new();

    public UpdateService() : this(null)
    {
    }

    /// <summary>Overload for diagnostics/tests that need to simulate an older build.</summary>
    public UpdateService(Version? currentVersion)
    {
        CurrentVersion = UpdateVersioning.Normalize(
            currentVersion ?? typeof(UpdateService).Assembly.GetName().Version ?? new Version(0, 2, 0));
    }

    public Version CurrentVersion { get; }

    public string CurrentVersionText => CurrentVersion.ToString(3);

    public UpdateRelease? Latest { get; private set; }

    public string? LastError { get; private set; }

    /// <summary>Inno Setup installs <c>unins000.exe</c>. Portable ZIP builds do not have it.</summary>
    public bool IsInstalledBuild =>
        File.Exists(Path.Combine(AppContext.BaseDirectory, "unins000.exe"));

    public string ReleasePageUrl => ReleasesPage;

    public async Task<UpdateCheckResult> CheckAsync(bool force = false, CancellationToken ct = default)
    {
        if (!force && _lastResult != null && DateTimeOffset.UtcNow - _lastCheckUtc < CacheWindow)
            return _lastResult;

        try
        {
            using var response = await Http.GetAsync(ApiLatest, HttpCompletionOption.ResponseHeadersRead, ct);
            response.EnsureSuccessStatusCode();

            await using var stream = await response.Content.ReadAsStreamAsync(ct);
            using var doc = await JsonDocument.ParseAsync(stream, cancellationToken: ct);
            var root = doc.RootElement;

            var tag = root.TryGetProperty("tag_name", out var tagElement) ? tagElement.GetString() : null;
            if (!UpdateVersioning.TryParseTag(tag, out var version))
                return Cache(UpdateCheckResult.Failed("The latest release has no recognizable version tag."));

            var releaseUrl = root.TryGetProperty("html_url", out var urlElement)
                ? urlElement.GetString() ?? ReleasesPage
                : ReleasesPage;

            string? installerUrl = null;
            string? installerName = null;
            string? shaUrl = null;
            long installerSize = 0;

            if (root.TryGetProperty("assets", out var assets) && assets.ValueKind == JsonValueKind.Array)
            {
                foreach (var asset in assets.EnumerateArray())
                {
                    var name = asset.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
                    var url = asset.TryGetProperty("browser_download_url", out var u) ? u.GetString() : null;
                    if (url == null) continue;

                    if (installerUrl == null &&
                        name.StartsWith("United-Setup-", StringComparison.OrdinalIgnoreCase) &&
                        name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
                    {
                        installerUrl = url;
                        installerName = name;
                        installerSize = asset.TryGetProperty("size", out var s) && s.TryGetInt64(out var size)
                            ? size
                            : 0;
                    }
                    else if (name.EndsWith(".exe.sha256", StringComparison.OrdinalIgnoreCase))
                    {
                        shaUrl = url;
                    }
                }
            }

            if (installerUrl == null || installerName == null)
                return Cache(UpdateCheckResult.Failed("The latest release does not contain a Setup EXE."));

            shaUrl ??= installerUrl + ".sha256";

            if (!UpdateVersioning.IsNewer(version, CurrentVersion))
            {
                Latest = null;
                LastError = null;
                return Cache(UpdateCheckResult.UpToDate(version));
            }

            Latest = new UpdateRelease(version, tag ?? $"v{version.ToString(3)}", releaseUrl,
                installerUrl, shaUrl, installerSize);
            LastError = null;
            AppLog.Info($"update available: {version.ToString(3)} (current {CurrentVersionText})");
            return Cache(new UpdateCheckResult(true, version, null));
        }
        catch (OperationCanceledException)
        {
            const string message = "Update check timed out.";
            LastError = message;
            AppLog.Warn(message);
            return Cache(UpdateCheckResult.Failed(message));
        }
        catch (Exception ex) when (ex is HttpRequestException or IOException or JsonException or InvalidOperationException)
        {
            LastError = ex.Message;
            AppLog.Warn($"update check failed: {ex.Message}");
            return Cache(UpdateCheckResult.Failed(ex.Message));
        }
    }

    private UpdateCheckResult Cache(UpdateCheckResult result)
    {
        _lastResult = result;
        _lastCheckUtc = DateTimeOffset.UtcNow;
        LastError = result.Error;
        return result;
    }

    /// <summary>Downloads the installer, checks the published SHA-256, and returns its path.</summary>
    public async Task<string> DownloadAndVerifyAsync(UpdateRelease release, IProgress<double>? progress = null,
        CancellationToken ct = default)
    {
        var dir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "United", "updates");
        Directory.CreateDirectory(dir);

        var target = Path.Combine(dir, $"United-Setup-{release.Version.ToString(3)}.exe");
        var expected = UpdateVersioning.ParseSha256(await Http.GetStringAsync(release.Sha256Url, ct));
        if (expected == null)
            throw new InvalidDataException("The release does not publish a valid SHA-256 checksum.");

        if (File.Exists(target) && await HashMatchesAsync(target, expected, ct))
        {
            progress?.Report(1);
            CleanupOldInstaller(dir, target);
            return target;
        }

        var temp = target + ".part";
        try
        {
            using var response = await Http.GetAsync(release.InstallerUrl, HttpCompletionOption.ResponseHeadersRead, ct);
            response.EnsureSuccessStatusCode();

            var total = response.Content.Headers.ContentLength ?? release.InstallerSize;
            await using (var input = await response.Content.ReadAsStreamAsync(ct))
            await using (var output = new FileStream(temp, FileMode.Create, FileAccess.Write, FileShare.None,
                             1 << 20, useAsync: true))
            {
                var buffer = new byte[81920];
                long read = 0;
                int count;
                while ((count = await input.ReadAsync(buffer, ct)) > 0)
                {
                    await output.WriteAsync(buffer.AsMemory(0, count), ct);
                    read += count;
                    if (total > 0) progress?.Report(Math.Clamp((double)read / total, 0, 1));
                }
            }

            if (!await HashMatchesAsync(temp, expected, ct))
                throw new InvalidDataException("The downloaded installer failed its SHA-256 check.");

            File.Move(temp, target, overwrite: true);
            progress?.Report(1);
            AppLog.Info($"update downloaded and verified: {target}");
            CleanupOldInstaller(dir, target);
            return target;
        }
        finally
        {
            try { if (File.Exists(temp)) File.Delete(temp); } catch { /* best effort */ }
        }
    }

    private static async Task<bool> HashMatchesAsync(string path, string expected, CancellationToken ct)
    {
        await using var stream = File.OpenRead(path);
        var actual = await SHA256.HashDataAsync(stream, ct);
        var actualHex = Convert.ToHexString(actual).ToLowerInvariant();
        return string.Equals(actualHex, expected, StringComparison.Ordinal);
    }

    private static void CleanupOldInstaller(string dir, string keep)
    {
        try
        {
            foreach (var file in Directory.EnumerateFiles(dir, "United-Setup-*.exe"))
                if (!string.Equals(file, keep, StringComparison.OrdinalIgnoreCase))
                    File.Delete(file);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Another process may still hold an old installer; harmless.
        }
    }

    /// <summary>
    /// Starts a hidden PowerShell helper that waits for this process to exit,
    /// runs the new installer silently, then reopens United.
    /// </summary>
    public bool BeginInstall(string installerPath)
    {
        if (!IsInstalledBuild)
        {
            AppLog.Warn("auto-update requested from a portable build; opening releases page instead");
            return false;
        }

        var appPath = Environment.ProcessPath ?? Path.Combine(AppContext.BaseDirectory, "United.exe");
        var scriptDir = Path.Combine(Path.GetTempPath(), "United");
        Directory.CreateDirectory(scriptDir);
        var scriptPath = Path.Combine(scriptDir, $"update-{Guid.NewGuid():N}.ps1");
        var logPath = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "United", "updates", "update.log");

        var script = $"""
            $ErrorActionPreference = 'SilentlyContinue'
            $log = '{Escape(logPath)}'
            New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null
            "[$(Get-Date -Format o)] update helper started" | Out-File $log -Append -Encoding utf8
            Wait-Process -Id {Environment.ProcessId} -Timeout 60 -ErrorAction SilentlyContinue
            Start-Sleep -Seconds 1
            $p = Start-Process -FilePath '{Escape(installerPath)}' -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-' -Wait -PassThru
            "[$(Get-Date -Format o)] installer exit $($p.ExitCode)" | Out-File $log -Append -Encoding utf8
            Start-Process -FilePath '{Escape(appPath)}'
            Remove-Item -LiteralPath $MyInvocation.MyCommand.Path -Force
            """;

        File.WriteAllText(scriptPath, script, new UTF8Encoding(false));
        var start = new ProcessStartInfo("powershell.exe")
        {
            UseShellExecute = true,
            WindowStyle = ProcessWindowStyle.Hidden,
            Arguments = $"-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{scriptPath}\"",
        };

        try
        {
            return Process.Start(start) != null;
        }
        catch (Exception ex)
        {
            AppLog.Warn($"could not start update helper: {ex.Message}");
            return false;
        }
    }

    private static string Escape(string value) => value.Replace("'", "''");

    private static HttpClient CreateHttpClient()
    {
        var client = new HttpClient { Timeout = TimeSpan.FromSeconds(20) };
        client.DefaultRequestHeaders.UserAgent.ParseAdd(
            $"United-Messenger/{typeof(UpdateService).Assembly.GetName().Version?.ToString(3) ?? "0.2"} " +
            "(+https://github.com/Zwart04/United)");
        client.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
        client.DefaultRequestHeaders.Add("X-GitHub-Api-Version", "2022-11-28");
        return client;
    }
}
