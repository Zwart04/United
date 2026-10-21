using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Text;
using United.Platform.Windows.Interop;

namespace United.Platform.Windows.Tray;

/// <summary>
/// System tray icon with unread badge, rendered on demand (no bitmap assets).
/// Menu: Open · Settings · Hibernate all · Quit.
/// </summary>
public sealed class TrayIconService : IDisposable
{
    private readonly NotifyIcon _icon;
    private readonly ContextMenuStrip _menu;
    private Icon? _currentIcon;
    private int _badge;
    private bool _disposed;

    public event Action? OpenRequested;
    public event Action? SettingsRequested;
    public event Action? HibernateAllRequested;
    public event Action? QuitRequested;

    public TrayIconService()
    {
        _menu = new ContextMenuStrip();
        var open = new ToolStripMenuItem("Open United") { Font = new Font(SystemFonts.DefaultFont, FontStyle.Bold) };
        open.Click += (_, _) => OpenRequested?.Invoke();
        var settings = new ToolStripMenuItem("Settings…");
        settings.Click += (_, _) => SettingsRequested?.Invoke();
        var hibernate = new ToolStripMenuItem("Hibernate all services");
        hibernate.Click += (_, _) => HibernateAllRequested?.Invoke();
        var quit = new ToolStripMenuItem("Quit");
        quit.Click += (_, _) => QuitRequested?.Invoke();

        _menu.Items.Add(open);
        _menu.Items.Add(new ToolStripSeparator());
        _menu.Items.Add(settings);
        _menu.Items.Add(hibernate);
        _menu.Items.Add(new ToolStripSeparator());
        _menu.Items.Add(quit);

        _icon = new NotifyIcon
        {
            Visible = true,
            ContextMenuStrip = _menu,
            Text = "United Messenger",
        };
        _icon.DoubleClick += (_, _) => OpenRequested?.Invoke();
        _icon.MouseClick += (_, e) =>
        {
            if (e.Button == MouseButtons.Left) OpenRequested?.Invoke();
        };

        ReplaceIcon(RenderIcon(0));
    }

    public void SetBadge(int unreadTotal)
    {
        if (_badge == unreadTotal) return;
        _badge = unreadTotal;
        ReplaceIcon(RenderIcon(unreadTotal));
        var text = unreadTotal > 0
            ? $"United Messenger — {unreadTotal} unread"
            : "United Messenger";
        // NotifyIcon.Text is limited to 127 chars.
        _icon.Text = text.Length > 127 ? text[..127] : text;
    }

    private void ReplaceIcon(Icon icon)
    {
        var old = _currentIcon;
        _currentIcon = icon;
        _icon.Icon = icon;
        if (old != null)
        {
            // NotifyIcon copies what it needs; free our handle on the next replacement.
            GC.SuppressFinalize(old);
            old.Dispose();
        }
    }

    /// <summary>32x32 (sharp on 200% DPI). Unread badge bottom-right.</summary>
    private static Icon RenderIcon(int unread)
    {
        const int size = 32;
        using var bmp = new Bitmap(size, size);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.TextRenderingHint = TextRenderingHint.ClearTypeGridFit;

            var rect = new Rectangle(1, 1, size - 3, size - 3);
            using (var brush = new SolidBrush(Color.FromArgb(31, 31, 31)))
                g.FillRoundedRectangle(brush, rect, 8);

            using (var font = new Font("Segoe UI", 15f, FontStyle.Bold, GraphicsUnit.Pixel))
            using (var brush = new SolidBrush(Color.White))
            {
                var fmt = new StringFormat { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
                g.DrawString("U", font, brush, new RectangleF(0, unread > 0 ? -2 : 0, size, size), fmt);
            }

            if (unread > 0)
            {
                var label = unread > 99 ? "9+" : unread.ToString();
                var badgeRect = new Rectangle(size - (label.Length >= 2 ? 17 : 14), size - (label.Length >= 2 ? 17 : 14),
                    label.Length >= 2 ? 16 : 13, label.Length >= 2 ? 14 : 13);
                using (var badgeBrush = new SolidBrush(Color.FromArgb(232, 17, 35)))
                    g.FillEllipse(badgeBrush, badgeRect);
                using (var font = new Font("Segoe UI", label.Length >= 2 ? 7.5f : 8.5f, FontStyle.Bold, GraphicsUnit.Pixel))
                using (var brush = new SolidBrush(Color.White))
                {
                    var fmt = new StringFormat { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
                    g.DrawString(label, font, brush, RectangleF.FromLTRB(badgeRect.X, badgeRect.Y, badgeRect.Right, badgeRect.Bottom), fmt);
                }
            }
        }

        var hIcon = bmp.GetHicon();
        try
        {
            using var tmp = Icon.FromHandle(hIcon);
            return (Icon)tmp.Clone();
        }
        finally
        {
            NativeMethods.DestroyIcon(hIcon);
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _icon.Visible = false;
        _icon.Dispose();
        _menu.Dispose();
        _currentIcon?.Dispose();
    }
}

internal static class GraphicsExtensions
{
    public static void FillRoundedRectangle(this Graphics g, Brush brush, Rectangle r, int radius)
    {
        var d = radius * 2;
        using var path = new GraphicsPath();
        path.AddArc(r.X, r.Y, d, d, 180, 90);
        path.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        path.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90);
        path.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        path.CloseFigure();
        g.FillPath(brush, path);
    }
}
