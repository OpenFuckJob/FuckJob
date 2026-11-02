param([int]$StartX, [int]$StartY, [int]$EndX, [int]$EndY)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OfferFlowMouse {
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
}
'@
$shell = New-Object -ComObject WScript.Shell
$cursor = [OfferFlowMouse+Point]::new()
$null = [OfferFlowMouse]::GetCursorPos([ref]$cursor)
$foreground = [OfferFlowMouse]::GetForegroundWindow()
$foregroundProcess = Get-Process | Where-Object { $_.MainWindowHandle -eq $foreground } | Select-Object -First 1
$browser = Get-Process chrome,msedge -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like '*滑动验证页面*' } |
    Select-Object -First 1
if (-not $browser) { throw '51job verification window absent' }
$browserWasMinimized = [OfferFlowMouse]::IsIconic($browser.MainWindowHandle)
try {
    $null = [OfferFlowMouse]::ShowWindow($browser.MainWindowHandle, 9)
    for ($attempt=0; $attempt -lt 10 -and [OfferFlowMouse]::GetForegroundWindow() -ne $browser.MainWindowHandle; $attempt++) {
        [OfferFlowMouse]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
        [OfferFlowMouse]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        $null = $shell.AppActivate($browser.MainWindowTitle)
        Start-Sleep -Milliseconds 150
    }
    if ([OfferFlowMouse]::GetForegroundWindow() -ne $browser.MainWindowHandle) { throw '51job verification window activation failed' }

    $rng = [Random]::new()
    for ($step=1; $step -le 60; $step++) {
        $t = $step / 60.0
        $eased = $t * $t * (3 - 2 * $t)
        $x = ($StartX + 450) - 450 * $eased
        $y = ($StartY - 190) + 190 * $eased + [Math]::Sin($t * [Math]::PI) * 8
        $null = [OfferFlowMouse]::SetCursorPos([Math]::Round($x), [Math]::Round($y))
        Start-Sleep -Milliseconds ($rng.Next(4,10))
    }
    Start-Sleep -Milliseconds 55
    [OfferFlowMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)

    $dx = @(2,4,12,20,38,68,81,112,138,174,214,239,275,310,357,372,587)
    $dy = @(0,0,-2,-2,-3,-4,-4,-3,-2,1,5,6,8,11,14,16,40)
    $wait = @(46,5,7,5,7,7,6,6,6,7,6,3,8,7,6,5,77)
    $width = $EndX - $StartX
    for ($i=0; $i -lt $dx.Count; $i++) {
        Start-Sleep -Milliseconds $wait[$i]
        $x = $StartX + $width * $dx[$i] / 587.0
        $y = $StartY + ($EndY - $StartY) * $dy[$i] / 40.0
        $null = [OfferFlowMouse]::SetCursorPos([Math]::Round($x), [Math]::Round($y))
    }
    [OfferFlowMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 500
}
finally {
    [OfferFlowMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    $null = [OfferFlowMouse]::SetCursorPos($cursor.X, $cursor.Y)
    if ($browserWasMinimized) { $null = [OfferFlowMouse]::ShowWindow($browser.MainWindowHandle, 6) }
    if ($foregroundProcess) { $null = $shell.AppActivate($foregroundProcess.Id) }
}
