using System.IO;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using United.App.Diagnostics;
using United.Core.Services;

namespace United.App.Shell;

/// <summary>
/// Resolves a service logo: a user-supplied image from %LOCALAPPDATA%\United\icons
/// first, then the bundled Brand.* vector, then the caller's monochrome glyph.
/// Custom images are normalized to a square-friendly PNG (max 256 px) so a single
/// predictable file exists per service and the sidebar stays crisp at any DPI.
/// </summary>
public static class IconStore
{
    public const int MaxDimension = 256;

    public static readonly string[] SupportedExtensions = { ".png", ".jpg", ".jpeg", ".ico", ".bmp", ".gif" };

    public const string FileDialogFilter =
        "Image files (*.png;*.jpg;*.jpeg;*.ico;*.bmp;*.gif)|*.png;*.jpg;*.jpeg;*.ico;*.bmp;*.gif|" +
        "PNG (*.png)|*.png|All files (*.*)|*.*";

    public static string DirectoryPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "United", "icons");

    /// <summary>Full path of the custom logo, or null when the bundled brand mark should be used.</summary>
    public static string? FindOverride(string serviceId)
    {
        foreach (var ext in SupportedExtensions)
        {
            var path = Path.Combine(DirectoryPath, serviceId + ext);
            if (File.Exists(path)) return path;
        }
        return null;
    }

    public static ImageSource? Resolve(ServiceDefinition def)
        => LoadOverride(def.Id) ?? ResolveBrand(def.Icon);

    public static ImageSource? ResolveBrand(string iconKey)
    {
        var key = "Brand." + Pascal(iconKey);
        try
        {
            return Application.Current?.TryFindResource(key) as ImageSource;
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"brand icon {key}");
            return null;
        }
    }

    public static ImageSource? LoadOverride(string serviceId)
    {
        var path = FindOverride(serviceId);
        if (path == null) return null;
        try
        {
            var image = LoadImage(path);
            image.Freeze();
            return image;
        }
        catch (Exception ex)
        {
            AppLog.Exception(ex, $"custom icon [{serviceId}] {path}");
            return null;
        }
    }

    /// <summary>Decodes any supported image (used by the picker preview too).</summary>
    public static BitmapSource LoadImage(string path)
    {
        using var stream = File.OpenRead(path);
        var decoder = BitmapDecoder.Create(
            stream,
            BitmapCreateOptions.PreservePixelFormat,
            BitmapCacheOption.OnLoad);

        // ICO files carry several sizes; use the largest square source.
        return decoder.Frames
            .OrderByDescending(f => (long)f.PixelWidth * f.PixelHeight)
            .First();
    }

    /// <summary>Copies an image into the icon store, downscaling it and normalizing to PNG.</summary>
    public static void SaveOverride(string serviceId, string sourcePath)
    {
        var source = LoadImage(sourcePath);
        if (source.PixelWidth <= 0 || source.PixelHeight <= 0)
            throw new InvalidOperationException("The image has no pixels.");

        var scaled = Downscale(source, MaxDimension);
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(scaled));

        Directory.CreateDirectory(DirectoryPath);
        var target = Path.Combine(DirectoryPath, serviceId + ".png");
        var tmp = target + ".tmp";
        using (var stream = File.Create(tmp)) encoder.Save(stream);
        File.Move(tmp, target, overwrite: true);

        // One canonical file per service: drop stale copies in other formats.
        foreach (var ext in SupportedExtensions)
        {
            if (string.Equals(ext, ".png", StringComparison.OrdinalIgnoreCase)) continue;
            try
            {
                var stale = Path.Combine(DirectoryPath, serviceId + ext);
                if (File.Exists(stale)) File.Delete(stale);
            }
            catch (IOException)
            {
                // best effort
            }
        }

        AppLog.Info($"custom logo saved [{serviceId}] {scaled.PixelWidth}x{scaled.PixelHeight} -> {target}");
    }

    public static void DeleteOverride(string serviceId)
    {
        foreach (var ext in SupportedExtensions)
        {
            try
            {
                var path = Path.Combine(DirectoryPath, serviceId + ext);
                if (File.Exists(path)) File.Delete(path);
            }
            catch (IOException ex)
            {
                AppLog.Exception(ex, $"delete icon [{serviceId}]");
            }
        }
    }

    private static BitmapSource Downscale(BitmapSource source, int max)
    {
        if (source.PixelWidth <= max && source.PixelHeight <= max) return source;

        var scale = Math.Min((double)max / source.PixelWidth, (double)max / source.PixelHeight);
        var resized = new TransformedBitmap(source, new ScaleTransform(scale, scale));
        resized.Freeze();
        return resized;
    }

    private static string Pascal(string key)
        => string.IsNullOrEmpty(key) ? "Globe" : char.ToUpperInvariant(key[0]) + key[1..];
}
