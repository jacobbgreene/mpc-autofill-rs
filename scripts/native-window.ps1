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
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$processCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $AppProcessId)
$nameCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Button)
$until = [DateTime]::UtcNow.AddSeconds(8)
do {
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $processCondition)
    foreach ($window in $windows) {
        $control = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $nameCondition)
        if ($null -ne $control) {
            $pattern = $control.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
            [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            $pattern.Invoke()
            exit 0
        }
    }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $until)
throw "Native dialog button not found: $Button"
