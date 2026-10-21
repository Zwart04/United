using System.IO;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using United.App.Diagnostics;
using United.Core.Services;

namespace United.App.Services;

/// <summary>
/// Reads and writes %LOCALAPPDATA%\United\services.json — the user's own service
/// definitions. Kept data-only: the file can still be edited by hand.
/// </summary>
public static class CustomServicesFile
{
    private static readonly JsonSerializerOptions ReadOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
    };

    private static readonly JsonSerializerOptions WriteOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        // Keep regexes readable when the user edits services.json by hand.
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    public static string DirectoryPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "United");

    public static string FilePath => Path.Combine(DirectoryPath, "services.json");

    public static List<ServiceDefinition> Load()
    {
        try
        {
            if (!File.Exists(FilePath)) return new List<ServiceDefinition>();
            var json = File.ReadAllText(FilePath);
            return JsonSerializer.Deserialize<List<ServiceDefinition>>(json, ReadOptions)
                   ?? new List<ServiceDefinition>();
        }
        catch (Exception ex) when (ex is IOException or JsonException)
        {
            AppLog.Exception(ex, "load services.json");
            return new List<ServiceDefinition>();
        }
    }

    public static void Save(IReadOnlyList<ServiceDefinition> services)
    {
        Directory.CreateDirectory(DirectoryPath);
        var json = JsonSerializer.Serialize(services, WriteOptions);
        var tmp = FilePath + ".tmp";
        File.WriteAllText(tmp, json);
        File.Move(tmp, FilePath, overwrite: true);
        AppLog.Info($"services.json saved ({services.Count} custom definition(s))");
    }

    public static void AddOrReplace(ServiceDefinition definition)
    {
        var all = Load();
        all.RemoveAll(s => string.Equals(s.Id, definition.Id, StringComparison.OrdinalIgnoreCase));
        all.Add(definition);
        Save(all);
    }

    public static bool Remove(string id)
    {
        var all = Load();
        var removed = all.RemoveAll(s => string.Equals(s.Id, id, StringComparison.OrdinalIgnoreCase)) > 0;
        if (removed) Save(all);
        return removed;
    }
}
