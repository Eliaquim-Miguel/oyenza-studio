# Smoke test on a real Windows desktop (used by CI):
# start Poni normally, screenshot the top of the screen while idle, hover the
# island to expand it, screenshot again, and record idle CPU / memory.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Out = "smoke-out"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Native {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[Native]::SetProcessDPIAware() | Out-Null

function Shot([string]$name, [int]$height = 360) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $h = [Math]::Min($height, $b.Height)
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
"screen: $($bounds.Width)x$($bounds.Height)" | Tee-Object -FilePath (Join-Path $Out "metrics.txt")

# Park the cursor away from the island.
[Native]::SetCursorPos([int]($bounds.Width / 2), [int]($bounds.Height / 2)) | Out-Null

$env:PONI_HOME = (Resolve-Path ".").Path + "\smoke-home"
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 4
Shot "1-idle"

# Idle CPU over 10 seconds (should be ~0).
$p.Refresh(); $cpu0 = $p.TotalProcessorTime.TotalMilliseconds
Start-Sleep -Seconds 10
$p.Refresh(); $cpu1 = $p.TotalProcessorTime.TotalMilliseconds
$ws = [Math]::Round($p.WorkingSet64 / 1MB, 1)
$priv = [Math]::Round($p.PrivateMemorySize64 / 1MB, 1)
"idle cpu ms over 10s: $([Math]::Round($cpu1 - $cpu0, 1))" | Tee-Object -FilePath (Join-Path $Out "metrics.txt") -Append
"working set MB: $ws; private MB: $priv" | Tee-Object -FilePath (Join-Path $Out "metrics.txt") -Append

# Hover the island (top centre) to expand it.
[Native]::SetCursorPos([int]($bounds.Width / 2), 3) | Out-Null
Start-Sleep -Milliseconds 120
[Native]::SetCursorPos([int]($bounds.Width / 2), 6) | Out-Null
Start-Sleep -Seconds 2
Shot "2-hover-expanded" 480

# Leave and let it collapse.
[Native]::SetCursorPos([int]($bounds.Width / 2), [int]($bounds.Height / 2)) | Out-Null
Start-Sleep -Seconds 2
Shot "3-collapsed-again"

$alive = -not $p.HasExited
"still running: $alive" | Tee-Object -FilePath (Join-Path $Out "metrics.txt") -Append
if ($alive) { Stop-Process -Id $p.Id -Force }

Copy-Item -Path (Join-Path $env:PONI_HOME "cache\poni.log") -Destination (Join-Path $Out "poni.log") -ErrorAction SilentlyContinue
if (-not $alive) { Write-Error "Poni exited during the smoke test"; exit 1 }
