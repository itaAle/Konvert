#Requires -Version 5.1
<#
.SYNOPSIS
    Watches the network activity of Konvert and of all its child processes.

.DESCRIPTION
    Polls the TCP connections and UDP endpoints owned by konvert.exe and by every
    descendant process (the WebView2 runtime runs in msedgewebview2.exe children),
    then prints a summary. Konvert is expected to show nothing: its UI talks to the
    Rust core through in-process IPC, and FFmpeg only reads/writes local files.

    Exit code: 0 if no socket was seen, 1 otherwise.

.PARAMETER Seconds
    How long to watch. Default: 30.

.PARAMETER Launch
    Optional path of konvert.exe to start before watching (closed at the end).

.PARAMETER ArgumentList
    Optional arguments for the launched program.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/monitor-network.ps1 -Launch target/release/konvert.exe
#>
[CmdletBinding()]
param(
    [int]$Seconds = 30,
    [string]$Launch,
    [string[]]$ArgumentList
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$started = $null
if ($Launch) {
    $startArgs = @{ FilePath = $Launch; PassThru = $true }
    if ($ArgumentList) { $startArgs.ArgumentList = $ArgumentList }
    $started = Start-Process @startArgs
    $rootIds = @($started.Id)
} else {
    $rootIds = @(Get-Process -Name konvert -ErrorAction SilentlyContinue | ForEach-Object Id)
    if ($rootIds.Count -eq 0) { throw 'Konvert is not running. Start it or use -Launch.' }
}

function Get-ProcessTree([int[]]$roots) {
    $all = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name)
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    foreach ($r in $roots) { [void]$ids.Add($r); $queue.Enqueue($r) }
    while ($queue.Count -gt 0) {
        $parent = $queue.Dequeue()
        foreach ($child in $all | Where-Object { $_.ParentProcessId -eq $parent }) {
            if ($ids.Add([int]$child.ProcessId)) { $queue.Enqueue([int]$child.ProcessId) }
        }
    }
    $all | Where-Object { $ids.Contains([int]$_.ProcessId) }
}

$seen = @{}
$maxProcesses = 0
$deadline = (Get-Date).AddSeconds($Seconds)
Write-Host "Watching Konvert's process tree for $Seconds s..."
while ((Get-Date) -lt $deadline) {
    $tree = @(Get-ProcessTree $rootIds)
    $maxProcesses = [Math]::Max($maxProcesses, $tree.Count)
    $names = @{}
    foreach ($p in $tree) { $names[[int]$p.ProcessId] = $p.Name }
    $ids = @($names.Keys)
    if ($ids.Count -gt 0) {
        foreach ($c in @(Get-NetTCPConnection -OwningProcess $ids -ErrorAction SilentlyContinue)) {
            $key = "TCP $($names[[int]$c.OwningProcess]) $($c.LocalAddress):$($c.LocalPort) -> $($c.RemoteAddress):$($c.RemotePort) [$($c.State)]"
            $seen[$key] = $true
        }
        foreach ($u in @(Get-NetUDPEndpoint -OwningProcess $ids -ErrorAction SilentlyContinue)) {
            $key = "UDP $($names[[int]$u.OwningProcess]) $($u.LocalAddress):$($u.LocalPort)"
            $seen[$key] = $true
        }
    }
    Start-Sleep -Milliseconds 500
}

if ($started -and -not $started.HasExited) { Stop-Process -Id $started.Id }

Write-Host "Processes observed in the tree (max): $maxProcesses"
if ($seen.Count -eq 0) {
    Write-Host 'OK: no TCP connection or UDP endpoint was opened by Konvert or its children.'
    exit 0
}
Write-Warning 'Sockets observed:'
$seen.Keys | Sort-Object | ForEach-Object { Write-Host "  $_" }
exit 1
