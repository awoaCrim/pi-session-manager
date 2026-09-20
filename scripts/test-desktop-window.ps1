param(
    [Parameter(Mandatory=$true)][int]$ProcessId,
    [Parameter(Mandatory=$true)][ValidateSet('State', 'Close', 'Minimize', 'Maximize', 'TrayClick', 'TrayMenu')][string]$Action,
    [string]$MenuText
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
# Native-only E2E driver. Never targets windows belonging to another process.
Add-Type @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class PiTestWindow {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr parameter);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder name, int length);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int length);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll", EntryPoint="PostMessageW")] public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr menu, int index);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetMenuString(IntPtr menu, uint item, StringBuilder text, int length, uint flags);
    public static IntPtr Find(int pid, string cls, string title) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((h, p) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            if (owner != pid) return true;
            if (cls == "#32768" && !IsWindowVisible(h)) return true;
            var c = new StringBuilder(256); GetClassName(h, c, c.Capacity);
            var t = new StringBuilder(512); GetWindowText(h, t, t.Capacity);
            if ((String.IsNullOrEmpty(cls) || c.ToString() == cls) && (String.IsNullOrEmpty(title) || t.ToString().StartsWith(title))) {
                result = h; return false;
            }
            return true;
        }, IntPtr.Zero);
        return result;
    }
    public static string Describe(int pid) {
        var windows = new List<string>();
        EnumWindows((h, p) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            if (owner == pid) {
                var c = new StringBuilder(256); GetClassName(h, c, c.Capacity);
                var t = new StringBuilder(512); GetWindowText(h, t, t.Capacity);
                windows.Add(h.ToString() + ": " + c + " / " + t);
            }
            return true;
        }, IntPtr.Zero);
        return String.Join("; ", windows);
    }
    public static string ItemText(IntPtr menu, uint index) {
        var text = new StringBuilder(256);
        GetMenuString(menu, index, text, text.Capacity, 0x400);
        return text.ToString();
    }

}
'@
# Match the application's per-monitor DPI mode when inspecting window bounds.
[PiTestWindow]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$window = [PiTestWindow]::Find($ProcessId, $null, 'Pi Sessions')
if ($window -eq [IntPtr]::Zero) { throw "Pi Sessions main window not found for PID ${ProcessId}: $([PiTestWindow]::Describe($ProcessId))" }
switch ($Action) {
    'State' {
        $rect = New-Object PiTestWindow+RECT
        [PiTestWindow]::GetWindowRect($window, [ref]$rect) | Out-Null
        @{
            visible = [PiTestWindow]::IsWindowVisible($window)
            minimized = [PiTestWindow]::IsIconic($window)
            maximized = [PiTestWindow]::IsZoomed($window)
            width = $rect.Right - $rect.Left
            height = $rect.Bottom - $rect.Top
        } | ConvertTo-Json -Compress
    }
    'Close' { [PiTestWindow]::PostMessage($window, 0x112, [IntPtr]0xf060, [IntPtr]::Zero) | Out-Null }
    'Minimize' { [PiTestWindow]::ShowWindow($window, 6) | Out-Null }
    'Maximize' { [PiTestWindow]::ShowWindow($window, 3) | Out-Null }
    default {
        $tray = [PiTestWindow]::Find($ProcessId, 'tray_icon_app', $null)
        if ($tray -eq [IntPtr]::Zero) { throw "Native tray window not found for PID $ProcessId" }
        # tray-icon 0.24 (Cargo.lock) receives Shell click notifications via message 6002.
        # Recheck this adapter when upgrading tray-icon. It drives the real tray
        # callback and popup, without adding a test IPC to the app.
        $mouseMessage = if ($Action -eq 'TrayClick') { 0x202 } else { 0x205 }
        [PiTestWindow]::PostMessage($tray, 6002, [IntPtr]::Zero, [IntPtr]$mouseMessage) | Out-Null
        if ($Action -eq 'TrayClick') { break }
        if (!$MenuText) { throw 'TrayMenu requires MenuText' }
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while ([DateTime]::UtcNow -lt $deadline) {
            $popup = [PiTestWindow]::Find($ProcessId, '#32768', $null)
            if ($popup -ne [IntPtr]::Zero) {
                $menu = [PiTestWindow]::SendMessage($popup, 0x1e1, [IntPtr]::Zero, [IntPtr]::Zero)
                if ($menu -ne [IntPtr]::Zero) { break }
            }
            Start-Sleep -Milliseconds 50
        }
        if (!$menu -or $menu -eq [IntPtr]::Zero) { throw 'Native tray context menu did not appear' }
        $items = @()
        $selected = -1
        for ($i = 0; $i -lt [PiTestWindow]::GetMenuItemCount($menu); $i++) {
            $text = [PiTestWindow]::ItemText($menu, $i)
            if ($text) { $items += $text }
            if ($text -eq $MenuText) { $selected = $i }
        }
        if ($selected -lt 0) { throw "Tray menu item '$MenuText' missing: $items" }
        # Resolve the real popup's command ID, dismiss it, and deliver the native
        # WM_COMMAND that Windows sends on selection. No production test hook or
        # screen coordinates are used, so DPI/focus cannot click another app.
        $commandId = [PiTestWindow]::GetMenuItemID($menu, $selected)
        if ($commandId -eq [uint32]::MaxValue) { throw 'Invalid native menu command ID' }
        [PiTestWindow]::SendMessage($tray, 0x1f, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        [PiTestWindow]::PostMessage($tray, 0x111, [IntPtr]$commandId, [IntPtr]::Zero) | Out-Null
        @{ items = $items; selected = $MenuText } | ConvertTo-Json -Compress
    }
}
