param(
    [Parameter(Mandatory=$true)][int]$AppProcessId,
    [ValidateSet('close', 'choose')][string]$Action = 'close',
    [string]$Button
)
$ErrorActionPreference = 'Stop'
$app = Get-Process -Id $AppProcessId
if ($Action -eq 'close') {
    [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    if (-not $app.CloseMainWindow()) { throw 'Native close request was not accepted' }
    exit 0
}
if ([string]::IsNullOrWhiteSpace($Button)) { throw 'An exact native button label is required' }
# TaskDialog controls remain native HWNDs even when the CI desktop does not
# expose a UI Automation provider. Never inspect or click another process.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class MpcNativeDialog {
    private delegate bool EnumCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumCallback callback, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern bool EnumChildWindows(IntPtr parent, EnumCallback callback, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr window, StringBuilder text, int capacity);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr window, StringBuilder text, int capacity);
    [DllImport("user32.dll")]
    private static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wParam,
        IntPtr lParam, uint flags, uint milliseconds, out UIntPtr result);

    public sealed class Control {
        public long Handle;
        public string ClassName;
        public string Text;
        public bool Enabled;
    }
    public sealed class Window {
        public Control Root;
        public List<Control> Buttons = new List<Control>();
    }
    private static bool BelongsTo(IntPtr window, uint processId) {
        uint owner;
        GetWindowThreadProcessId(window, out owner);
        return owner == processId;
    }
    private static Control Describe(IntPtr window) {
        var text = new StringBuilder(2048);
        var name = new StringBuilder(256);
        GetWindowText(window, text, text.Capacity);
        GetClassName(window, name, name.Capacity);
        return new Control { Handle = window.ToInt64(), ClassName = name.ToString(),
            Text = text.ToString(), Enabled = IsWindowEnabled(window) };
    }
    public static List<Window> Snapshot(uint processId) {
        var windows = new List<Window>();
        EnumWindows(delegate(IntPtr window, IntPtr unused) {
            if (!BelongsTo(window, processId)) return true;
            var snapshot = new Window { Root = Describe(window) };
            EnumChildWindows(window, delegate(IntPtr child, IntPtr unusedChild) {
                if (BelongsTo(child, processId)) {
                    var control = Describe(child);
                    if (String.Equals(control.ClassName, "Button", StringComparison.OrdinalIgnoreCase))
                        snapshot.Buttons.Add(control);
                }
                return true;
            }, IntPtr.Zero);
            windows.Add(snapshot);
            return true;
        }, IntPtr.Zero);
        return windows;
    }
    public static void Click(uint processId, long handle, string label) {
        var window = new IntPtr(handle);
        var control = Describe(window);
        if (!BelongsTo(window, processId) || !control.Enabled ||
            !String.Equals(control.ClassName, "Button", StringComparison.OrdinalIgnoreCase) ||
            !String.Equals(control.Text, label, StringComparison.Ordinal))
            throw new InvalidOperationException("The expected native button changed before invocation");
        UIntPtr result;
        // BM_CLICK, SMTO_ABORTIFHUNG: deliver an actual native button click,
        // bounded independently of the five-second process-exit assertion.
        if (SendMessageTimeout(window, 0x00F5, UIntPtr.Zero, IntPtr.Zero, 0x0002, 1000, out result) == IntPtr.Zero)
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Native button invocation failed");
    }
}
'@
$until = [DateTime]::UtcNow.AddSeconds(8)
do {
    $windows = [MpcNativeDialog]::Snapshot($AppProcessId)
    foreach ($window in $windows) {
        foreach ($control in $window.Buttons) {
            if ($control.Enabled -and $control.Text -ceq $Button) {
                [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
                [MpcNativeDialog]::Click($AppProcessId, $control.Handle, $Button)
                exit 0
            }
        }
    }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $until)
$app.Refresh()
$diagnostics = [ordered]@{
    processId = $AppProcessId
    exited = $app.HasExited
    windows = @([MpcNativeDialog]::Snapshot($AppProcessId))
} | ConvertTo-Json -Depth 6 -Compress
throw "Native dialog button not found: $Button. App-specific native controls: $diagnostics"
