# Toggles hibernation of a sidebar service through its context menu (UI Automation).
# Used by the benchmark procedure so measurements do not depend on manual clicks.
#   .\toggle-hibernate.ps1                 # first sidebar item
#   .\toggle-hibernate.ps1 -Index 2
#   .\toggle-hibernate.ps1 -Action Resume  # only invoke Resume, fail if absent
param(
    [int]$Index = 0,
    [ValidateSet("Toggle", "Hibernate", "Resume")]
    [string]$Action = "Toggle",
    [int]$WaitSeconds = 6
)

$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Native {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
"@
[Native]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms

$p = Get-Process United -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Output "United window not found"; exit 1 }
$main = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)

# Dismiss any open native menu, then bring United to the foreground.
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
[Native]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 500

$radioCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::RadioButton)
$mr = $main.Current.BoundingRectangle
$radios = @($main.FindAll([System.Windows.Automation.TreeScope]::Descendants, $radioCond) |
    Where-Object { $_.Current.BoundingRectangle.X -lt ($mr.X + 70) } |
    Sort-Object { $_.Current.BoundingRectangle.Y })
if ($radios.Count -le $Index) { Write-Output "sidebar item $Index not found"; exit 1 }

$b = $radios[$Index].Current.BoundingRectangle
[Native]::SetCursorPos([int]($b.X + $b.Width / 2), [int]($b.Y + $b.Height / 2)) | Out-Null
Start-Sleep -Milliseconds 300
[Native]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
[Native]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 1000

$menuCond = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $p.Id)),
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)))
$items = @([System.Windows.Automation.AutomationElement]::RootElement.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants, $menuCond))
$names = @($items | ForEach-Object { $_.Current.Name })
Write-Output ("menu: " + ($names -join " | "))

$wanted = switch ($Action) {
    "Hibernate" { @("Hibernate") }
    "Resume"    { @("Resume") }
    default     { @("Hibernate", "Resume") }
}
$target = $items | Where-Object { $wanted -contains $_.Current.Name } | Select-Object -First 1
if (-not $target) {
    Write-Output "requested menu item not found"
    [System.Windows.Forms.SendKeys]::SendWait("{ESC}")
    exit 1
}

$target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
Write-Output ("invoked: " + $target.Current.Name)
Start-Sleep -Seconds $WaitSeconds
