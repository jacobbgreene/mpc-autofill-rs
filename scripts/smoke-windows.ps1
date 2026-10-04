$ErrorActionPreference = 'Stop'
$installers = @(Get-ChildItem 'target/release/bundle/nsis/*.exe')
if ($installers.Count -ne 1) { throw 'Expected one Windows installer' }
$installDir = Join-Path $env:RUNNER_TEMP 'mpc-autofill-smoke'
$installer = Start-Process -FilePath $installers[0].FullName -ArgumentList @('/S', "/D=$installDir") -WindowStyle Hidden -Wait -PassThru
if ($installer.ExitCode -ne 0) { throw "Installer failed: $($installer.ExitCode)" }
$binary = Join-Path $installDir 'mpc-autofill.exe'
if (-not (Test-Path $binary)) { throw "Installed executable missing: $binary" }
$app = Start-Process -FilePath $binary -WindowStyle Hidden -PassThru
try {
    $ready = $false
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        Start-Sleep -Seconds 1
        $app.Refresh()
        if ($app.HasExited) { throw "App exited during startup: $($app.ExitCode)" }
        if ($app.MainWindowTitle -eq 'MPC Autofill') { $ready = $true; break }
    }
    if (-not $ready) { throw 'App did not create its main window' }
    Start-Sleep -Seconds 5
    $app.Refresh()
    if ($app.HasExited) { throw 'App exited after creating its window' }
    if (-not $app.CloseMainWindow()) { throw 'App did not accept the native window close request' }
    if (-not $app.WaitForExit(5000)) { throw 'App did not exit within five seconds of native close' }
    if ($app.ExitCode -ne 0) { throw "App failed during shutdown: $($app.ExitCode)" }
    Write-Output 'PASS: Windows installer completed; installed app opened and closed cleanly.'
} finally {
    if (-not $app.HasExited) { Stop-Process -Id $app.Id }
}
