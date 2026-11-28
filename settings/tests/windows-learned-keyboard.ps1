param([string]$Executable = (Join-Path $PSScriptRoot '../target/release/pangyi-settings.exe'))

# Run with Windows PowerShell 5.1 on an interactive desktop.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class LearnedTestMouse {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int x, int y, int data, UIntPtr extra);
}
'@
[LearnedTestMouse]::SetProcessDPIAware() | Out-Null

$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$tempRoot = (Resolve-Path -LiteralPath $env:TEMP).Path
$profile = (New-Item -ItemType Directory -Path (Join-Path $tempRoot ('pangyi-learned-tab-' + [guid]::NewGuid()))).FullName
$data = (New-Item -ItemType Directory -Path (Join-Path $profile Pangyi)).FullName
$rows = 1..20 | ForEach-Object { "词$([char](0x4e00 + $_))" + [char]9 + $_ }
[IO.File]::WriteAllLines((Join-Path $data learned.txt), [string[]]$rows, [Text.UTF8Encoding]::new($false))

function Find-Control([string]$Name) {
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::NameProperty, $Name
    )
    $element = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
    if (!$element) { throw "Missing Windows UI Automation element: $Name" }
    $element
}

$app = $null
try {
    $previousLocal = $env:LOCALAPPDATA
    try {
        $env:LOCALAPPDATA = $profile
        $app = Start-Process -FilePath $executablePath -PassThru
    } finally {
        $env:LOCALAPPDATA = $previousLocal
    }
    $app.WaitForInputIdle(5000) | Out-Null
    Start-Sleep -Milliseconds 750
    $app.Refresh()
    $desktop = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
    $width = [Math]::Min(1240, $desktop.Width)
    $height = [Math]::Min(1000, $desktop.Height)
    [LearnedTestMouse]::SetWindowPos($app.MainWindowHandle, [IntPtr](-1), $desktop.X, $desktop.Y, $width, $height, 0x40) | Out-Null
    $window = [System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
    $nav = (Find-Control '词库').Current.BoundingRectangle
    [LearnedTestMouse]::SetCursorPos([int]($nav.X + 10), [int]($nav.Y + $nav.Height / 2)) | Out-Null
    [LearnedTestMouse]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [LearnedTestMouse]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 250
    $rect = $window.Current.BoundingRectangle
    [LearnedTestMouse]::SetCursorPos([int]($rect.X + 244), [int]($rect.Bottom - 125)) | Out-Null
    Start-Sleep -Milliseconds 180
    foreach ($i in 1..2) {
        [LearnedTestMouse]::mouse_event(0x800, 0, 0, -720, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 400
    }
    $expand = Find-Control '查看常用词'
    $expand.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Start-Sleep -Milliseconds 400
    [LearnedTestMouse]::mouse_event(0x800, 0, 0, -720, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    (Find-Control "加入我的词库，词$([char]0x4e14)，使用 20 次").SetFocus()
    1..38 | ForEach-Object {
        [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
        Start-Sleep -Milliseconds 60
    }
    Start-Sleep -Milliseconds 500
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement.Current
    if ($focused.ProcessId -ne $app.Id -or $focused.Name -ne "加入我的词库，词$([char]0x4e01)，使用 1 次") {
        throw "Tab did not reach the last learned word: $($focused.Name)"
    }
    [System.Windows.Forms.SendKeys]::SendWait('+{TAB}')
    Start-Sleep -Milliseconds 450
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement.Current
    if ($focused.Name -ne "忽略 词$([char]0x4e02)，使用 2 次") {
        throw "Shift+Tab did not return to the previous row: $($focused.Name)"
    }
    'Tab reached row 20 and Shift+Tab returned to row 19.'
} finally {
    if ($app -and !$app.HasExited) {
        $app.CloseMainWindow() | Out-Null
        $app.WaitForExit(2500) | Out-Null
    }
    $resolvedProfile = (Resolve-Path -LiteralPath $profile).Path
    $prefix = $tempRoot.TrimEnd([char]'\') + '\pangyi-learned-tab-'
    if ($resolvedProfile.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $resolvedProfile -Recurse -Force
    }
}
