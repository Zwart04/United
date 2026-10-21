# Dev verification helper: launches United, switches services through UI Automation,
# captures DPI-aware window screenshots via PrintWindow (no foreground stealing),
# exercises the Settings > Services page and the hibernate/resume overlay.
param(
    [string]$Exe = "",
    [string]$OutDir = "$env:TEMP\opencode",
    [int]$StartupWaitSec = 12,
    [int]$SwitchWaitSec = 5
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class UiShotNative {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[UiShotNative]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null  # PER_MONITOR_AWARE_V2

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

function Capture-Window([IntPtr]$hwnd, [string]$path) {
    $r = New-Object UiShotNative+RECT
    if (-not [UiShotNative]::GetWindowRect($hwnd, [ref]$r)) { Write-Output "  rect failed"; return }
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) { Write-Output "  zero window"; return }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [UiShotNative]::PrintWindow($hwnd, $hdc, 2) | Out-Null  # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "  saved $path (${w}x${h})"
}

function Find-Descendant($root, $controlType, [string]$name) {
    $cond = New-Object System.Windows.Automation.AndCondition(
        (New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty, $controlType)),
        (New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::NameProperty, $name)))
    return $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
}

function Invoke-Element($element, [string]$label) {
    if (-not $element) { Write-Output "  $label not found"; return $false }
    try {
        $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Write-Output "  invoked $label"
        return $true
    } catch {
        try {
            $element.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
            Write-Output "  selected $label"
            return $true
        } catch {
            Write-Output "  $label action failed: $($_.Exception.Message)"
            return $false
        }
    }
}

if ($Exe) {
    Get-Process United -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe)
    Start-Sleep -Seconds $StartupWaitSec
} else {
    Start-Sleep -Seconds 1
}

$p = Get-Process United -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } | Sort-Object StartTime -Desc | Select-Object -First 1
if (-not $p) { Write-Output "United window not found"; exit 1 }

$main = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
$mr = $main.Current.BoundingRectangle
Write-Output ("main window: {0},{1} {2}x{3} title='{4}'" -f $mr.X, $mr.Y, $mr.Width, $mr.Height, $main.Current.Name)

Capture-Window $p.MainWindowHandle (Join-Path $OutDir "main-whatsapp.png")

# Sidebar items are RadioButtons in the left rail (x below the content area).
$radioCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::RadioButton)
$radios = @($main.FindAll([System.Windows.Automation.TreeScope]::Descendants, $radioCond))
$sidebar = @($radios | Where-Object { $_.Current.BoundingRectangle.X -lt ($mr.X + 70) } | Sort-Object { $_.Current.BoundingRectangle.Y })
Write-Output "sidebar radio buttons: $($sidebar.Count)"
for ($i = 0; $i -lt $sidebar.Count; $i++) {
    $b = $sidebar[$i].Current.BoundingRectangle
    Write-Output ("  [{0}] name='{1}' at {2},{3}" -f $i, $sidebar[$i].Current.Name, $b.X, $b.Y)
}

if ($sidebar.Count -ge 2) {
    $target = $sidebar[1]  # Instagram
    try {
        $target.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Write-Output "selected sidebar item #2 (Instagram)"
    } catch {
        try {
            $target.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
            Write-Output "toggled sidebar item #2 (Instagram)"
        } catch { Write-Output "switch failed: $($_.Exception.Message)" }
    }
    Start-Sleep -Seconds $SwitchWaitSec
    Capture-Window $p.MainWindowHandle (Join-Path $OutDir "main-instagram.png")

    # Back to WhatsApp so hibernation targets the active, loaded service.
    try {
        $sidebar[0].GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Write-Output "selected sidebar item #1 (WhatsApp)"
    } catch { Write-Output "switch back failed: $($_.Exception.Message)" }
    Start-Sleep -Seconds $SwitchWaitSec
}

# Open Settings: the gear button is the leftmost button in the sidebar rail.
$btnCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Button)
$buttons = @($main.FindAll([System.Windows.Automation.TreeScope]::Descendants, $btnCond))
$gear = $buttons |
    Where-Object { $_.Current.BoundingRectangle.X -lt ($mr.X + 70) -and $_.Current.BoundingRectangle.Width -gt 20 } |
    Sort-Object { $_.Current.BoundingRectangle.Y } -Descending | Select-Object -First 1

$settingsWin = $null
if ($gear) {
    try {
        $gear.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Write-Output "settings invoked"
        Start-Sleep -Seconds 2
        $pidCond = New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $p.Id)
        $windows = @([System.Windows.Automation.AutomationElement]::RootElement.FindAll(
            [System.Windows.Automation.TreeScope]::Children, $pidCond))
        $settingsWin = $windows | Where-Object { $_.Current.Name -like "*Settings*" } | Select-Object -First 1
        if ($settingsWin) {
            Capture-Window ([IntPtr]$settingsWin.Current.NativeWindowHandle) (Join-Path $OutDir "settings-general.png")
        } else {
            Write-Output "settings window not found; windows: $($windows | ForEach-Object { $_.Current.Name } )"
        }
    } catch { Write-Output "settings open failed: $($_.Exception.Message)" }
} else {
    Write-Output "gear button not found"
}

if ($settingsWin) {
    $navServices = Find-Descendant $settingsWin ([System.Windows.Automation.ControlType]::RadioButton) "Services"
    if (Invoke-Element $navServices "Services nav") {
        Start-Sleep -Seconds 1
        Capture-Window ([IntPtr]$settingsWin.Current.NativeWindowHandle) (Join-Path $OutDir "settings-services.png")

        $hibernateButtons = @($settingsWin.FindAll(
            [System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.AndCondition(
                (New-Object System.Windows.Automation.PropertyCondition(
                    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
                    [System.Windows.Automation.ControlType]::Button)),
                (New-Object System.Windows.Automation.PropertyCondition(
                    [System.Windows.Automation.AutomationElement]::NameProperty, "Hibernate"))))))
        Write-Output "hibernate buttons: $($hibernateButtons.Count)"
        if ($hibernateButtons.Count -gt 0) {
            Invoke-Element $hibernateButtons[0] "Hibernate (WhatsApp)" | Out-Null
            Start-Sleep -Seconds 2
            Capture-Window ([IntPtr]$settingsWin.Current.NativeWindowHandle) (Join-Path $OutDir "settings-services-hibernated.png")
        }

        $navPerf = Find-Descendant $settingsWin ([System.Windows.Automation.ControlType]::RadioButton) "Performance"
        if (Invoke-Element $navPerf "Performance nav") {
            Start-Sleep -Seconds 2
            Capture-Window ([IntPtr]$settingsWin.Current.NativeWindowHandle) (Join-Path $OutDir "settings-performance.png")
        }
        $navAbout = Find-Descendant $settingsWin ([System.Windows.Automation.ControlType]::RadioButton) "About"
        if (Invoke-Element $navAbout "About nav") {
            Start-Sleep -Milliseconds 500
            Capture-Window ([IntPtr]$settingsWin.Current.NativeWindowHandle) (Join-Path $OutDir "settings-about.png")
        }

        # Close only the settings window, keep the main app running.
        [UiShotNative]::PostMessage([IntPtr]$settingsWin.Current.NativeWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Seconds 1
        Capture-Window $p.MainWindowHandle (Join-Path $OutDir "main-hibernated.png")

        $resume = Find-Descendant $main ([System.Windows.Automation.ControlType]::Button) "Resume"
        if (Invoke-Element $resume "Resume") {
            Start-Sleep -Seconds ($SwitchWaitSec + 8)
            Capture-Window $p.MainWindowHandle (Join-Path $OutDir "main-resumed.png")
        }

        # Context menu on the sidebar. A click on the title bar makes the app foreground
        # (SetForegroundWindow alone is refused), then right-click the first service.
        if ($sidebar.Count -gt 0) {
            $mr = $main.Current.BoundingRectangle
            [UiShotNative]::SetCursorPos([int]($mr.X + 300), [int]($mr.Y + 20)) | Out-Null
            Start-Sleep -Milliseconds 250
            [UiShotNative]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            [UiShotNative]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 500

            $b = $sidebar[0].Current.BoundingRectangle
            [UiShotNative]::SetCursorPos([int]($b.X + $b.Width / 2), [int]($b.Y + $b.Height / 2)) | Out-Null
            Start-Sleep -Milliseconds 300
            [UiShotNative]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
            [UiShotNative]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 900

            $menuCond = New-Object System.Windows.Automation.AndCondition(
                (New-Object System.Windows.Automation.PropertyCondition(
                    [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $p.Id)),
                (New-Object System.Windows.Automation.PropertyCondition(
                    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
                    [System.Windows.Automation.ControlType]::MenuItem)))
            $menuItems = @([System.Windows.Automation.AutomationElement]::RootElement.FindAll(
                [System.Windows.Automation.TreeScope]::Descendants, $menuCond))
            Write-Output "context menu items: $($menuItems.Count)"
            if ($menuItems.Count -gt 0) {
                $left = ($menuItems | ForEach-Object { $_.Current.BoundingRectangle.X } | Measure-Object -Minimum).Minimum
                $top = ($menuItems | ForEach-Object { $_.Current.BoundingRectangle.Y } | Measure-Object -Minimum).Minimum
                $right = ($menuItems | ForEach-Object { $_.Current.BoundingRectangle.Right } | Measure-Object -Maximum).Maximum
                $bottom = ($menuItems | ForEach-Object { $_.Current.BoundingRectangle.Bottom } | Measure-Object -Maximum).Maximum
                $pad = 14
                $w = [int]($right - $left) + 2 * $pad
                $h = [int]($bottom - $top) + 2 * $pad
                $bmp = New-Object System.Drawing.Bitmap($w, $h)
                $g = [System.Drawing.Graphics]::FromImage($bmp)
                $g.CopyFromScreen([int]$left - $pad, [int]$top - $pad, 0, 0, (New-Object System.Drawing.Size($w, $h)))
                $g.Dispose()
                $menuPath = Join-Path $OutDir "main-contextmenu.png"
                $bmp.Save($menuPath, [System.Drawing.Imaging.ImageFormat]::Png)
                $bmp.Dispose()
                Write-Output "  saved $menuPath (${w}x${h})"
                foreach ($mi in $menuItems) { Write-Output ("  menu item: '{0}'" -f $mi.Current.Name) }
                [System.Windows.Forms.SendKeys]::SendWait("{ESC}")
            } else {
                Capture-Window $p.MainWindowHandle (Join-Path $OutDir "main-contextmenu-failed.png")
            }
        }
    }
}

Write-Output "done"
