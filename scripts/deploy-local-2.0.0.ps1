param(
    [switch]$PrepareOnly
)

$ErrorActionPreference = 'Stop'
$sourceRoot = 'D:\github\CodexPlusPlus-rollout-compression-toggle-20261027'
$installRoot = 'D:\Codex++'
$sourceExe = Join-Path $sourceRoot 'dist\jacy-build\2.0.0\codex-plus-plus.exe'
$expectedHash = '99D256F9A463ABAE454CE7C2FEE8100C39D2BB8A0F4CF405F719DB5C70BFC09E'
$version = '2.0.0'
$settingsPath = 'C:\Users\18127\.codex-session-delete\settings.json'
$reportPath = 'D:\codex_cli\reports\codex-plus-plus-2.0.0-deployment.json'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexPlusPlus'

if (!(Test-Path -LiteralPath $sourceExe)) { throw "Build artifact missing: $sourceExe" }
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $sourceExe).Hash -ne $expectedHash) { throw 'Build artifact hash mismatch.' }
$pe = (Get-Item -LiteralPath $sourceExe).VersionInfo
if ($pe.FileVersion -ne $version -or $pe.ProductVersion -ne $version) { throw 'Build artifact version mismatch.' }
if (!(Test-Path -LiteralPath $installRoot)) { throw 'Existing install directory missing.' }
$registration = Get-ItemProperty -LiteralPath $uninstallKey
if ((Resolve-Path -LiteralPath $registration.InstallLocation).ProviderPath -ne (Resolve-Path -LiteralPath $installRoot).ProviderPath) { throw 'Existing installation registration mismatch.' }

$manager = @(Get-Process -Name 'codex-plus-plus' -ErrorAction SilentlyContinue)
$official = @(Get-Process -Name 'ChatGPT','Codex' -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*\WindowsApps\OpenAI.Codex_*\app\*' } | Select-Object Id,StartTime,Path)
$settingsHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $settingsPath).Hash

if ($PrepareOnly) {
    [ordered]@{ stage = 'ready'; version = $version; source = $sourceExe; sha256 = $expectedHash; managerRunning = ($manager.Count -gt 0); officialAppCount = $official.Count } | ConvertTo-Json
    exit 0
}

$backupRoot = Join-Path 'D:\codex_cli\backups' ('codex-plus-plus-before-2.0.0-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
foreach ($name in @('codex-plus-plus.exe','uninstall.exe','codex-plus-plus.runtime-gate.json')) {
    $item = Join-Path $installRoot $name
    if (Test-Path -LiteralPath $item) { Copy-Item -LiteralPath $item -Destination (Join-Path $backupRoot $name) }
}
Copy-Item -LiteralPath $settingsPath -Destination (Join-Path $backupRoot 'settings.json')
[ordered]@{ DisplayVersion = $registration.DisplayVersion; InstallLocation = $registration.InstallLocation } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backupRoot 'uninstall-version-before.json') -Encoding utf8

if ($manager.Count -gt 0) {
    $manager | Stop-Process -Force
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 250
        $manager = @(Get-Process -Name 'codex-plus-plus' -ErrorAction SilentlyContinue)
    } while ($manager.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)
    if ($manager.Count -gt 0) { throw 'Codex++ manager did not exit.' }
}

$report = [ordered]@{
    stage = 'backed-up'
    version = $version
    sourceCommit = '19190433764a4c2d7e10e43e14faa181c771e14d'
    deployedAt = [DateTimeOffset]::Now.ToString('o')
    backup = $backupRoot
    priorHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $installRoot 'codex-plus-plus.exe')).Hash
    expectedHash = $expectedHash
    settingsHashBefore = $settingsHash
    officialAppBefore = $official
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding utf8

Copy-Item -LiteralPath $sourceExe -Destination (Join-Path $installRoot 'codex-plus-plus.exe') -Force
if ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $installRoot 'codex-plus-plus.exe')).Hash -ne $expectedHash) { throw 'Installed hash mismatch.' }
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $settingsPath).Hash -ne $settingsHash) { throw 'Settings changed during deployment.' }
Set-ItemProperty -LiteralPath $uninstallKey -Name 'DisplayVersion' -Value $version
$started = Start-Process -FilePath (Join-Path $installRoot 'codex-plus-plus.exe') -WorkingDirectory $installRoot -WindowStyle Hidden -PassThru
$report.stage = 'started'
$report.managerPid = $started.Id
$report.installedHash = $expectedHash
$report.settingsHashAfter = (Get-FileHash -Algorithm SHA256 -LiteralPath $settingsPath).Hash
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding utf8
$report | ConvertTo-Json -Depth 8
