Add-Type @'
using System;
using System.Runtime.InteropServices;
public class FW {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int L,T,R,B; }
}
'@
$p = Get-Process aurora | Select-Object -First 1
$h = $p.MainWindowHandle
[FW]::ShowWindow($h, 9) | Out-Null
[FW]::BringWindowToTop($h) | Out-Null
[FW]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 800
$r = New-Object FW+RECT
[FW]::GetWindowRect($h, [ref]$r) | Out-Null
Write-Output ("rect " + $r.L + "," + $r.T + " " + ($r.R - $r.L) + "x" + ($r.B - $r.T))
