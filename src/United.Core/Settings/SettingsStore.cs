using System.Text.Json;

namespace United.Core.Settings;

/// <summary>
/// JSON settings store with atomic writes and corruption tolerance.
/// Location: %LOCALAPPDATA%\United\settings.json
/// </summary>
public sealed class SettingsStore
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
        // Window geometry uses NaN as an "unset" sentinel.
        NumberHandling = System.Text.Json.Serialization.JsonNumberHandling.AllowNamedFloatingPointLiterals,
    };

    private readonly string _path;
    private readonly object _gate = new();

    public SettingsStore(string path)
    {
        _path = path;
    }

    public static string DefaultPath =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "United", "settings.json");

    public string FilePath => _path;

    public AppSettings Load()
    {
        lock (_gate)
        {
            try
            {
                if (!File.Exists(_path)) return Migrate(new AppSettings());
                var json = File.ReadAllText(_path);
                return Migrate(JsonSerializer.Deserialize<AppSettings>(json, JsonOptions) ?? new AppSettings());
            }
            catch (Exception ex) when (ex is JsonException or IOException)
            {
                TryQuarantineCorruptFile();
                return Migrate(new AppSettings());
            }
        }
    }

    /// <summary>One-time, idempotent upgrades of settings written by older builds.</summary>
    private static AppSettings Migrate(AppSettings settings)
    {
        if (settings.SettingsVersion < 1)
        {
            // v1: Facebook joined the default sidebar. Insert it after Instagram so
            // existing installations pick it up without losing their order.
            settings.EnabledServices ??= new List<string>();
            if (!settings.EnabledServices.Any(id => string.Equals(id, "facebook", StringComparison.OrdinalIgnoreCase)))
            {
                var index = settings.EnabledServices.FindIndex(id =>
                    string.Equals(id, "instagram", StringComparison.OrdinalIgnoreCase));
                settings.EnabledServices.Insert(index >= 0 ? index + 1 : settings.EnabledServices.Count, "facebook");
            }
            settings.SettingsVersion = 1;
        }
        return settings;
    }

    public void Save(AppSettings settings)
    {
        lock (_gate)
        {
            var dir = Path.GetDirectoryName(_path);
            if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
            var json = JsonSerializer.Serialize(settings, JsonOptions);
            var tmp = _path + ".tmp";
            File.WriteAllText(tmp, json);
            File.Move(tmp, _path, overwrite: true);
        }
    }

    private void TryQuarantineCorruptFile()
    {
        try
        {
            if (File.Exists(_path))
                File.Move(_path, _path + ".bad", overwrite: true);
        }
        catch (IOException)
        {
            // best effort — never crash over diagnostics
        }
    }
}
