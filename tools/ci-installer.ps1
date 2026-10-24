# Installer check on a real Windows desktop (used by CI): install the previous release, upgrade it with the new
# NSIS installer through its pages (a screenshot of each), check what landed on disk and in the registry, start
# the app, then uninstall. Screenshots are best effort; the checks decide the exit code.
param(
    [Parameter(Mandatory = $true)][string]$Setup,
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Previous = "",
    [string]$Out = "installer-out"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Native {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
"@
[Native]::SetProcessDPIAware() | Out-Null

$results = [System.Collections.Generic.List[object]]::new()
function Check([string]$name, [bool]$ok, [string]$detail = "") {
    $results.Add([pscustomobject]@{ check = $name; ok = $ok; detail = $detail })
    "{0}  {1}  {2}" -f ($(if ($ok) { "PASS" } else { "FAIL" }), $name, $detail) | Write-Host
}

function Shot([string]$name, [System.Drawing.Rectangle]$r) {
    $bmp = New-Object System.Drawing.Bitmap $r.Width, $r.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

function WindowShot([string]$name, [IntPtr]$h) {
    $r = New-Object Native+RECT
    if ($h -ne [IntPtr]::Zero -and [Native]::GetWindowRect($h, [ref]$r)) {
        Shot $name ([System.Drawing.Rectangle]::FromLTRB($r.Left - 8, $r.Top - 8, $r.Right + 8, $r.Bottom + 8))
    } else {
        Shot $name ([System.Windows.Forms.Screen]::PrimaryScreen.Bounds)
    }
}

function WaitFor([scriptblock]$cond, [int]$seconds) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $end) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 250 }
    return $false
}

$title = "Oyen Poni Setup"
$programs = [Environment]::GetFolderPath("Programs")
$installDir = Join-Path $env:LOCALAPPDATA "Programs\Poni"
$exe = Join-Path $installDir "poni.exe"
$dataDir = Join-Path $env:APPDATA "Poni"
$marker = Join-Path $dataDir "ci-marker.txt"
$uninstKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Poni"

function PoniEntries {
    $roots = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall", "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall"
    @($roots | ForEach-Object { Get-ChildItem $_ -ErrorAction SilentlyContinue } |
        ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -like "*Poni*" })
}

# 1. The previous release, the way people have it today.
if ($Previous) {
    Start-Process -FilePath $Previous -ArgumentList "/S" -Wait
    Check "previous release installed" (Test-Path $exe) $Previous
    Check "previous Start menu shortcut" (Test-Path (Join-Path $programs "Poni.lnk"))
    New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
    Set-Content -Path $marker -Value "settings and data must survive the upgrade"
}

# 2. The new installer, page by page.
$p = Start-Process -FilePath $Setup -PassThru
$h = [IntPtr]::Zero
$shown = WaitFor { $script:h = [Native]::FindWindow($null, $title); $script:h -ne [IntPtr]::Zero } 30
Check "installer window opens" $shown "title '$title'"
$walked = $false
if ($shown) {
    Start-Sleep -Seconds 1
    WindowShot "1-welcome" $h
    $pages = @("2-components", "3-finish")
    foreach ($page in $pages) {
        [Native]::SetForegroundWindow($h) | Out-Null
        Start-Sleep -Milliseconds 300
        [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
        Start-Sleep -Seconds $(if ($page -eq "3-finish") { 6 } else { 2 })
        WindowShot $page $h
    }
    # Finish, with "Launch Oyen Poni now" ticked.
    [Native]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
    $walked = WaitFor { $p.HasExited } 30
}
if (-not $walked) {
    # The pages didn't take the keys: finish the same upgrade silently so the checks below still run.
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    Start-Process -FilePath $Setup -ArgumentList "/S" -Wait
}
Check "walked through the installer pages" $walked "otherwise installed with /S"

# 3. What the upgrade left behind.
$entry = Get-ItemProperty $uninstKey -ErrorAction SilentlyContinue
Check "Installed apps name" ($entry.DisplayName -eq "Oyen Poni") "$($entry.DisplayName)"
Check "Installed apps version" ($entry.DisplayVersion -eq $Version) "$($entry.DisplayVersion)"
$entries = PoniEntries
Check "one Installed apps entry" ($entries.Count -eq 1) (($entries | ForEach-Object { $_.DisplayName }) -join ", ")
Check "new Start menu shortcut" (Test-Path (Join-Path $programs "Oyen Poni.lnk"))
Check "old Start menu shortcut removed" (-not (Test-Path (Join-Path $programs "Poni.lnk")))
$vi = (Get-Item $exe -ErrorAction SilentlyContinue).VersionInfo
Check "poni.exe product name" ($vi.ProductName -eq "Oyen Poni") "$($vi.ProductName)"
Check "poni.exe version" ($vi.ProductVersion -eq $Version) "$($vi.ProductVersion)"
if ($Previous) { Check "data kept through the upgrade" (Test-Path $marker) $marker }
try {
    [System.Drawing.Icon]::ExtractAssociatedIcon($exe).ToBitmap().Save((Join-Path $Out "exe-icon.png"), [System.Drawing.Imaging.ImageFormat]::Png)
} catch { Write-Host "icon extract failed: $_" }

# 4. The app itself starts and stays up (launched by Finish, or started here).
$running = WaitFor { @(Get-Process poni -ErrorAction SilentlyContinue).Count -gt 0 } 8
if (-not $running) { Start-Process -FilePath $exe; $running = WaitFor { @(Get-Process poni -ErrorAction SilentlyContinue).Count -gt 0 } 8 }
Start-Sleep -Seconds 4
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Shot "4-island" ([System.Drawing.Rectangle]::new($screen.Left, $screen.Top, $screen.Width, [Math]::Min(240, $screen.Height)))
Shot "5-tray" ([System.Drawing.Rectangle]::new($screen.Right - 520, $screen.Bottom - 120, 520, 120))
Check "app running after install" (@(Get-Process poni -ErrorAction SilentlyContinue).Count -gt 0)
Get-Process poni -ErrorAction SilentlyContinue | Stop-Process -Force

# 5. Uninstall: the app and shortcuts go, settings and data stay.
Start-Process -FilePath (Join-Path $installDir "Uninstall.exe") -ArgumentList "/S" -Wait
$gone = WaitFor { -not (Test-Path $exe) } 30
Check "uninstall removes the app" $gone
Check "uninstall removes the Installed apps entry" (-not (Test-Path $uninstKey))
Check "uninstall removes the shortcut" (-not (Test-Path (Join-Path $programs "Oyen Poni.lnk")))
if ($Previous) { Check "uninstall keeps settings and data" (Test-Path $marker) }

$results | ConvertTo-Json | Set-Content (Join-Path $Out "results.json")
$failed = @($results | Where-Object { -not $_.ok -and $_.check -ne "walked through the installer pages" })
if ($failed.Count -gt 0) { Write-Error "$($failed.Count) installer check(s) failed"; exit 1 }
