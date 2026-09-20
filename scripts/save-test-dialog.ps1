param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public static class PiTestDialog {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr param);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd, StringBuilder name, int length);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr ReadText(IntPtr hwnd, uint message, IntPtr length, StringBuilder text);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr SetText(IntPtr hwnd, uint message, IntPtr value, string text);
    [DllImport("user32.dll", EntryPoint="PostMessageW")] public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr hwnd);
    public static IntPtr[] Children(IntPtr parent) {
        var handles = new List<IntPtr>();
        EnumChildWindows(parent, (h, p) => { handles.Add(h); return true; }, IntPtr.Zero);
        return handles.ToArray();
    }
    public static string Text(IntPtr handle) { var text = new StringBuilder(2048); ReadText(handle, 0x000D, (IntPtr)2048, text); return text.ToString(); }
    public static string Class(IntPtr handle) { var text = new StringBuilder(256); GetClassName(handle, text, 256); return text.ToString(); }
}
'@
$title = [string][char]0x5bfc + [char]0x51fa + ' Pi ' + [char]0x4f1a + [char]0x8bdd
$deadline = [DateTime]::UtcNow.AddSeconds(20)
$edit = [IntPtr]::Zero
$save = [IntPtr]::Zero
while ([DateTime]::UtcNow -lt $deadline) {
    $dialog = [PiTestDialog]::FindWindow('#32770', $title)
    if ($dialog -ne [IntPtr]::Zero) {
        foreach ($child in [PiTestDialog]::Children($dialog)) {
            $class = [PiTestDialog]::Class($child)
            if ($class -eq 'Edit' -and [PiTestDialog]::Text($child) -like 'pi-session-*.jsonl') { $edit = $child }
            if ($class -eq 'Button' -and [PiTestDialog]::GetDlgCtrlID($child) -eq 1) { $save = $child }
        }
    }
    if ($edit -ne [IntPtr]::Zero -and $save -ne [IntPtr]::Zero) { break }
    Start-Sleep -Milliseconds 150
}
if ($edit -eq [IntPtr]::Zero -or $save -eq [IntPtr]::Zero) { throw 'Pi native Save controls did not appear' }
[PiTestDialog]::SetText($edit, 0x000C, [IntPtr]::Zero, $Destination) | Out-Null
[PiTestDialog]::PostMessage($save, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Write-Output 'Native Save dialog completed'
