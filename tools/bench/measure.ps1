# Measures United plus its entire WebView2 process tree.
# Usage:
#   .\measure.ps1                       # one snapshot of the running app
#   .\measure.ps1 -Label "1 active" -Seconds 15
#   .\measure.ps1 -Json out.json        # machine-readable snapshot
#   .\measure.ps1 -Launch -Exe ..\..\src\United.App\bin\Debug\net10.0-windows10.0.19041.0\United.exe
#
# Memory numbers are PrivateMemorySize64 (commit charge) and WorkingSet64, summed
# over United.exe, its WebView2 browser process and all descendants. Private
# commit is the number used in the benchmark report; Task Manager's "Memory"
# column is private working set and will be lower.
param(
    [string]$Label = "snapshot",
    [int]$Seconds = 0,
    [string]$Json = "",
    [switch]$Launch,
    [string]$Exe = ""
)

$ErrorActionPreference = "Stop"

function Get-UnitedTree {
    $root = Get-Process United -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } |
        Sort-Object StartTime -Descending |
        Select-Object -First 1
    if (-not $root) {
        $root = Get-Process United -ErrorAction SilentlyContinue |
            Sort-Object StartTime -Descending |
            Select-Object -First 1
    }
    if (-not $root) { return @() }

    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name
    $ids = New-Object System.Collections.Generic.HashSet[int]
    [void]$ids.Add($root.Id)
    $added = $true
    while ($added) {
        $added = $false
        foreach ($p in $all) {
            if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) {
                [void]$ids.Add([int]$p.ProcessId)
                $added = $true
            }
        }
    }

    $rows = foreach ($id in $ids) {
        $proc = Get-Process -Id $id -ErrorAction SilentlyContinue
        if (-not $proc) { continue }
        $cim = $all | Where-Object ProcessId -eq $id | Select-Object -First 1
        [PSCustomObject]@{
            Pid         = $proc.Id
            Name        = $proc.ProcessName
            Parent      = if ($cim) { $cim.ParentProcessId } else { 0 }
            PrivateMB   = [math]::Round($proc.PrivateMemorySize64 / 1MB, 1)
            WorkingSetMB= [math]::Round($proc.WorkingSet64 / 1MB, 1)
            CpuSeconds  = [math]::Round($proc.TotalProcessorTime.TotalSeconds, 1)
            Threads     = $proc.Threads.Count
        }
    }
    return @($rows | Sort-Object PrivateMB -Descending)
}

if ($Launch) {
    if (-not $Exe) { throw "-Launch requires -Exe" }
    Get-Process United -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 2
    Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe)
    Start-Sleep -Seconds 15
}

$first = @(Get-UnitedTree)
if ($first.Count -eq 0) {
    Write-Output "United is not running."
    exit 2
}

$cpuBefore = ($first | Measure-Object CpuSeconds -Sum).Sum
$sleep = if ($Seconds -gt 0) { $Seconds } else { 0 }
if ($sleep -gt 0) { Start-Sleep -Seconds $sleep }

$tree = @(Get-UnitedTree)
$cpuAfter = ($tree | Measure-Object CpuSeconds -Sum).Sum
$cores = [Environment]::ProcessorCount
$cpuPercent = if ($sleep -gt 0) {
    [math]::Round((($cpuAfter - $cpuBefore) / $sleep / $cores) * 100, 2)
} else { $null }

$sumPrivate = [math]::Round((($tree | Measure-Object PrivateMB -Sum).Sum), 1)
$sumWorking = [math]::Round((($tree | Measure-Object WorkingSetMB -Sum).Sum), 1)
$webviewCount = @($tree | Where-Object Name -eq "msedgewebview2").Count
$started = (Get-Process -Id $tree[0].Pid).StartTime
if (-not $started) { $started = (Get-Process -Id $tree[-1].Pid).StartTime }

$result = [PSCustomObject]@{
    Label           = $Label
    Timestamp       = (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
    TreeProcesses   = $tree.Count
    WebView2Procs   = $webviewCount
    PrivateMB       = $sumPrivate
    WorkingSetMB    = $sumWorking
    CpuPercent      = $cpuPercent
    SampleSeconds   = $sleep
    Processes       = $tree
}

Write-Output ""
Write-Output ("== {0} ==" -f $Label)
Write-Output ("time: {0}" -f $result.Timestamp)
Write-Output ("processes: {0} (WebView2: {1})" -f $result.TreeProcesses, $result.WebView2Procs)
Write-Output ("private commit: {0} MB" -f $result.PrivateMB)
Write-Output ("working set:    {0} MB" -f $result.WorkingSetMB)
if ($null -ne $cpuPercent) {
    Write-Output ("cpu:            {0}% of machine ({1} logical cores, {2}s sample)" -f $cpuPercent, $cores, $sleep)
}
Write-Output ""
$tree | Format-Table Pid, Name, PrivateMB, WorkingSetMB, Threads -AutoSize

if ($Json) {
    $dir = Split-Path $Json -Parent
    if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    $result | ConvertTo-Json -Depth 5 | Set-Content -Path $Json -Encoding UTF8
    Write-Output "json: $Json"
}
