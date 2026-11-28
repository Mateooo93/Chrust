# Captures screenshots of Chrust for the README.
#
#   pwsh tools/screenshots/capture.ps1 -Exe target/release/chrust-port.exe -Out docs/screenshots
#
# Each shot is its own run against a throwaway profile whose session is written
# first, so the tab strip and the active tab are exactly what the shot needs.
# Driving the window with keystrokes was tried and is flaky: a frameless window
# does not always take keyboard focus from a script.
param(
    [string]$Exe = "target/release/chrust-port.exe",
    [string]$Out = "docs/screenshots",
    [string]$DataDir = "$env:TEMP\chrust-shots",
    [int]$Width = 1400,
    [int]$Height = 900
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[Win]::SetProcessDPIAware() | Out-Null

function Save-WindowPng {
    param([IntPtr]$Handle, [string]$Path)
    # The client area, not the frame: Chrust draws its own title bar, and this
    # leaves out the resize border, the drop shadow and the taskbar.
    $r = New-Object Win+RECT
    [Win]::GetClientRect($Handle, [ref]$r) | Out-Null
    $w = $r.Right - $r.Left
    $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) { throw "the window has no size" }
    $origin = New-Object Win+POINT
    [Win]::ClientToScreen($Handle, [ref]$origin) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($origin.X, $origin.Y, 0, 0, (New-Object System.Drawing.Size $w, $h))
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Output ("  " + (Split-Path $Path -Leaf) + "  ${w}x${h}")
}

$work = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
$shotW = [Math]::Min($Width, $work.Width - 40)
$shotH = [Math]::Min($Height, $work.Height - 40)

$strip = @(
    "https://example.com/",
    "https://www.rust-lang.org/",
    "https://en.wikipedia.org/",
    "http://aurora.newtab/",
    "http://aurora.settings/"
)

$shots = @(
    @{ Name = "browser.png";  Tabs = @($strip[0], $strip[1], $strip[2]); Active = 0 },
    @{ Name = "new-tab.png";  Tabs = @($strip[3], $strip[0], $strip[1], $strip[2]); Active = 0 },
    @{ Name = "settings.png"; Tabs = @($strip[4], $strip[0], $strip[1]); Active = 0 }
)

if (-not (Test-Path $Out)) { New-Item -ItemType Directory -Force $Out | Out-Null }

foreach ($shot in $shots) {
    # A directory of its own per shot: the engine keeps files open after the
    # browser exits, and deleting the previous one failed on them.
    $dir = Join-Path $DataDir ([System.IO.Path]::GetFileNameWithoutExtension($shot.Name))
    New-Item -ItemType Directory -Force $dir | Out-Null
    $session = @{ windows = @(@{ tabs = $shot.Tabs; active = $shot.Active }) } | ConvertTo-Json -Depth 5
    # No BOM: the browser parses this with serde_json, which rejects one.
    [System.IO.File]::WriteAllText((Join-Path $dir "session.json"), $session)

    $env:CHRUST_DATA_DIR = $dir
    # One quiet run first: it fills the site-icon cache and the engine's own
    # caches, so the shot does not show placeholder globes for the tabs that
    # never became active.
    $warm = Start-Process -FilePath $Exe -PassThru
    Start-Sleep -Seconds 8
    $warm.CloseMainWindow() | Out-Null
    Start-Sleep -Seconds 2
    if (-not $warm.HasExited) { $warm.Kill() }
    Start-Sleep -Milliseconds 500

    $proc = Start-Process -FilePath $Exe -PassThru
    $deadline = (Get-Date).AddSeconds(30)
    $handle = [IntPtr]::Zero
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 400
        $proc.Refresh()
        if ($proc.MainWindowHandle -ne [IntPtr]::Zero) { $handle = $proc.MainWindowHandle; break }
    }
    if ($handle -eq [IntPtr]::Zero) { throw "chrust never showed a window" }

    # Inside the desktop: a window over the taskbar captures the taskbar.
    [Win]::MoveWindow($handle, $work.X + 20, $work.Y + 20, $shotW, $shotH, $true) | Out-Null
    # Restored and brought to the front: shown without activating, the window
    # was never painted and `CopyFromScreen` captured whatever window was on
    # top instead. The cost is that the shot window has focus while it is up,
    # so do not type anywhere else during a capture run.
    [Win]::ShowWindow($handle, 9) | Out-Null
    [Win]::SetForegroundWindow($handle) | Out-Null
    # A cold engine showed tabs 2 and 3 as still loading at seven seconds;
    # with the profile warmed this is comfortably past every page.
    Start-Sleep -Seconds 9
    Save-WindowPng $handle (Join-Path $Out $shot.Name)

    $proc.CloseMainWindow() | Out-Null
    Start-Sleep -Seconds 2
    if (-not $proc.HasExited) { $proc.Kill() }
    Start-Sleep -Milliseconds 500
}

Write-Output "done"
