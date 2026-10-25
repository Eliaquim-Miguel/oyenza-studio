<#
Oyen Poni over a game: does Windows keep sending the game's frames straight
to the screen while the readout shows?

Double-click "Run overlay check.cmd" (next to this file). It asks Windows for
admin (PresentMon needs it), downloads Intel's PresentMon 2.6.0 next to this
file if it isn't there (checked against its SHA-256), finds the game you're
playing, then measures five set-ups for 45 seconds each:

  1. Poni hidden       Show over games off, island hidden over games (baseline)
  2. Island on top     the island over the game (Windows composes it: the cost)
  3. Corner            the corner readout, see-through
  4. Corner opaque     the corner readout on a solid background
  5. Corner click-thru the corner readout, clicks pass through it

"Straight to screen" is the share of the game's frames Windows showed without
composing them (PresentMon's "Hardware: Independent Flip" or "Hardware
Composed: Independent Flip"). A corner style that keeps it as high as run 1
adds no delay. Poni's own row says whether Windows gave the corner window a
hardware overlay plane ("Hardware Composed: Independent Flip").

Your settings.json is put back exactly as it was at the end.
#>
param(
    [string] $Game = "",      # the game's .exe name; found by itself when empty
    [int] $Seconds = 45,      # length of each recording
    [int] $Settle = 12        # wait before each recording
)
$ErrorActionPreference = "Stop"

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    $elevate = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"", "-Seconds", $Seconds, "-Settle", $Settle)
    if ($Game) { $elevate += @("-Game", $Game) }
    Start-Process powershell -Verb RunAs -ArgumentList $elevate
    exit
}

function Pause-End([string] $msg) {
    if ($msg) { Write-Host $msg -ForegroundColor Yellow }
    Read-Host "Press Enter to close"
    exit
}

Write-Host "Oyen Poni overlay check" -ForegroundColor Cyan
$here = Split-Path -Parent $PSCommandPath

# PresentMon 2.6.0 (Intel, MIT), the console build.
$pmUrl = "https://github.com/GameTechDev/PresentMon/releases/download/v2.6.0/PresentMon-2.6.0-x64.exe"
$pmSha = "B2A706BC6AD475749E3B7E3409263AA1E6906D45BDCF993F6DBC0F660188F1AF"
$pm = Join-Path $here "PresentMon-2.6.0-x64.exe"
if (-not (Test-Path $pm) -or (Get-FileHash $pm -Algorithm SHA256).Hash -ne $pmSha) {
    Write-Host "Downloading PresentMon 2.6.0 from GitHub..."
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    try { Invoke-WebRequest -Uri $pmUrl -OutFile $pm -UseBasicParsing } catch { Pause-End "Couldn't download PresentMon: $($_.Exception.Message)" }
    if ((Get-FileHash $pm -Algorithm SHA256).Hash -ne $pmSha) { Remove-Item $pm -ErrorAction SilentlyContinue; Pause-End "The PresentMon download didn't match its fingerprint; not running it." }
}

$poni = Get-Process -Name poni -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $poni) { Pause-End "Start Oyen Poni (the test build) first, then run this again." }
$poniExe = $poni.Path
if ($poniExe -like "*WindowsApps*") { Pause-End "This is the Store build; use the test build." }

$settings = Join-Path $env:APPDATA "Poni\settings.json"
$poniLog = Join-Path $env:LOCALAPPDATA "Poni\poni.log"
$out = Join-Path $here ("overlay-check-" + (Get-Date).ToUniversalTime().ToString("yyyyMMdd-HHmm"))
New-Item -ItemType Directory -Path $out -Force | Out-Null
$hadSettings = Test-Path $settings
if ($hadSettings) { Copy-Item $settings (Join-Path $out "settings.backup.json") }

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class PoniWin {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
"@

function Stop-Poni {
    Get-Process -Name poni -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.Id }
    Start-Sleep -Seconds 2
}

function Start-Poni {
    # Through explorer, so Poni runs as you and not elevated like this window.
    Start-Process -FilePath "explorer.exe" -ArgumentList "`"$poniExe`""
    for ($i = 0; $i -lt 20 -and -not (Get-Process -Name poni -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
    Start-Sleep -Seconds 3
}

function Set-PoniSettings([bool] $hide, [bool] $game, [string] $readout) {
    if ($hadSettings) {
        $s = Get-Content -Raw (Join-Path $out "settings.backup.json") | ConvertFrom-Json
    } else {
        $s = [pscustomobject]@{ settings_version = 3 }
    }
    $s | Add-Member -NotePropertyName theme -NotePropertyValue "classic" -Force
    $s | Add-Member -NotePropertyName hide_in_fullscreen -NotePropertyValue $hide -Force
    $s | Add-Member -NotePropertyName game_stats -NotePropertyValue $game -Force
    $s | Add-Member -NotePropertyName game_readout -NotePropertyValue $readout -Force
    [IO.File]::WriteAllText($settings, ($s | ConvertTo-Json -Depth 20), (New-Object Text.UTF8Encoding $false))
}

# Frames that reached the screen without Windows composing them.
$direct = @("Hardware: Independent Flip", "Hardware Composed: Independent Flip", "Hardware: Legacy Flip", "Hardware: Legacy Copy to front buffer")

function Find-Game {
    Write-Host ""
    Write-Host "Switch to your game now (borderless full-screen) and leave it; I'll look for it in 15 s." -ForegroundColor Cyan
    Start-Sleep -Seconds 15
    [console]::Beep(800, 150)
    $csv = Join-Path $out "find-game.csv"
    $p = Start-Process -FilePath $pm -PassThru -WindowStyle Hidden -ArgumentList @("--output_file", "`"$csv`"", "--timed", 6, "--terminate_after_timed", "--stop_existing_session", "--no_console_stats", "--exclude", "dwm.exe", "--exclude", "poni.exe")
    $p.WaitForExit()
    if (-not (Test-Path $csv)) { return $null }
    $skip = @("explorer.exe", "msedge.exe", "chrome.exe", "firefox.exe", "discord.exe", "steamwebhelper.exe", "searchhost.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe", "textinputhost.exe")
    $top = Import-Csv $csv | Where-Object { $skip -notcontains $_.Application.ToLower() } | Group-Object Application | Sort-Object Count -Descending | Select-Object -First 1
    if ($top) { return $top.Name } else { return $null }
}

function Measure-Run([string] $name) {
    $csv = Join-Path $out "$name.csv"
    $logStart = if (Test-Path $poniLog) { (Get-Item $poniLog).Length } else { 0 }
    Write-Host ""
    Write-Host "== $name : be in the game; recording starts in $Settle s ==" -ForegroundColor Cyan
    Start-Sleep -Seconds $Settle
    [console]::Beep(1000, 150); [console]::Beep(1000, 150)
    $p = Start-Process -FilePath $pm -PassThru -WindowStyle Hidden -ArgumentList @(
        "--process_name", $Game, "--process_name", "poni.exe", "--output_file", "`"$csv`"", "--timed", $Seconds,
        "--terminate_after_timed", "--stop_existing_session", "--no_console_stats")
    $islandSeen = $false; $cornerSeen = $false
    while (-not $p.HasExited) {
        $h = [PoniWin]::FindWindowW("Poni.Island", $null); if ($h -ne [IntPtr]::Zero -and [PoniWin]::IsWindowVisible($h)) { $islandSeen = $true }
        $c = [PoniWin]::FindWindowW("Poni.Corner", $null); if ($c -ne [IntPtr]::Zero -and [PoniWin]::IsWindowVisible($c)) { $cornerSeen = $true }
        Start-Sleep -Seconds 2
    }
    [console]::Beep(600, 600)
    if ((Test-Path $poniLog) -and (Get-Item $poniLog).Length -gt $logStart) {
        $fs = [IO.File]::Open($poniLog, 'Open', 'Read', 'ReadWrite'); $fs.Seek($logStart, 'Begin') | Out-Null
        (New-Object IO.StreamReader($fs)).ReadToEnd() -split "`r?`n" | Where-Object { $_ -match "show over games|corner|fullscreen|frames" } | Set-Content (Join-Path $out "$name.poni-log.txt")
        $fs.Close()
    }
    return Summarize $name $csv $islandSeen $cornerSeen
}

function Summarize([string] $name, [string] $csv, [bool] $island, [bool] $corner) {
    if (-not (Test-Path $csv)) { Write-Warning "$name : PresentMon wrote nothing"; return $null }
    $rows = Import-Csv $csv
    $gameRows = @($rows | Where-Object { $_.Application -ieq $Game })
    $poniRows = @($rows | Where-Object { $_.Application -ieq "poni.exe" })
    if ($gameRows.Count -lt 10) { Write-Warning "$name : only $($gameRows.Count) game frames"; return $null }
    $cols = $gameRows[0].PSObject.Properties.Name
    $ftCol = @("MsBetweenPresents", "FrameTime") | Where-Object { $cols -contains $_ } | Select-Object -First 1
    $latCol = @("DisplayLatency", "MsUntilDisplayed") | Where-Object { $cols -contains $_ } | Select-Object -First 1
    $ft = $gameRows | ForEach-Object { $_.$ftCol } | Where-Object { $_ -and $_ -ne "NA" } | ForEach-Object { [double]$_ } | Sort-Object
    $lat = if ($latCol) { @($gameRows | ForEach-Object { $_.$latCol } | Where-Object { $_ -and $_ -ne "NA" } | ForEach-Object { [double]$_ }) } else { @() }
    $avgFt = ($ft | Measure-Object -Average).Average
    $p99 = $ft[[math]::Min($ft.Count - 1, [math]::Floor($ft.Count * 0.99))]
    $straight = @($gameRows | Where-Object { $direct -contains $_.PresentMode }).Count
    $mode = $gameRows | Group-Object PresentMode | Sort-Object Count -Descending | Select-Object -First 1
    $poniMode = if ($poniRows.Count) { ($poniRows | Group-Object PresentMode | Sort-Object Count -Descending | Select-Object -First 1).Name } else { "-" }
    [pscustomobject]@{
        Run                  = $name
        "Avg FPS"            = [math]::Round(1000 / $avgFt, 1)
        "1% low"             = [math]::Round(1000 / $p99, 1)
        "Latency ms"         = if ($lat.Count) { [math]::Round(($lat | Measure-Object -Average).Average, 2) } else { "n/a" }
        "Straight to screen" = "$([math]::Round(100 * $straight / $gameRows.Count))%"
        "Game present mode"  = $mode.Name
        "Shown"              = if ($corner) { "corner" } elseif ($island) { "island" } else { "nothing" }
        "Poni's window"      = $poniMode
    }
}

if (-not $Game) { $Game = Find-Game }
if (-not $Game) { Pause-End "I couldn't find a game drawing frames. Start the game in borderless full-screen and run this again." }
Write-Host "Game: $Game" -ForegroundColor Green

$runs = @(
    @{ Name = "1-poni-hidden";      Hide = $true;  Game = $false; Readout = "peek" },
    @{ Name = "2-island-on-top";    Hide = $false; Game = $false; Readout = "peek" },
    @{ Name = "3-corner";           Hide = $false; Game = $true;  Readout = "corner" },
    @{ Name = "4-corner-opaque";    Hide = $false; Game = $true;  Readout = "corner_opaque" },
    @{ Name = "5-corner-clickthru"; Hide = $false; Game = $true;  Readout = "corner_layered" }
)
$results = @()
try {
    foreach ($r in $runs) {
        Stop-Poni
        Set-PoniSettings $r.Hide $r.Game $r.Readout
        Start-Poni
        $res = Measure-Run $r.Name
        if ($res) { $results += $res }
    }
} finally {
    Stop-Poni
    if ($hadSettings) { Copy-Item (Join-Path $out "settings.backup.json") $settings -Force }
    elseif (Test-Path $settings) { Remove-Item $settings }
    Start-Poni
}

$summary = Join-Path $out "summary.txt"
$results | Format-Table -AutoSize | Out-String -Width 220 | Set-Content $summary
@(
    "Game: $Game",
    "Straight to screen = frames Windows showed without composing them. Run 1 is the target; run 2 shows the cost of a window over the game.",
    "Poni's window 'Hardware Composed: Independent Flip' = Windows gave the corner readout a hardware overlay plane."
) | Add-Content $summary
Get-Content $summary | Write-Host
Write-Host ""
Write-Host "Saved in $out. Send summary.txt to Claude." -ForegroundColor Green
Start-Process notepad.exe -ArgumentList "`"$summary`""
Pause-End ""
