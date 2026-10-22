param(
    [string]$SourceRoot = 'D:\github\CodexPlusPlus-upstream-integration-20261022',
    [Parameter(Mandatory)][string]$ExpectedCommit,
    [string]$Version = '1.11.1'
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$ProgressPreference = 'SilentlyContinue'
$validationRoot = Join-Path $SourceRoot "build-validation\$Version"
New-Item -ItemType Directory -Path $validationRoot -Force | Out-Null
$checks = [ordered]@{}
function Set-Stage([string]$Stage, [object]$Detail) {
    @{stage=$Stage; source=$ExpectedCommit; version=$Version; checks=$checks; detail=$Detail; updatedAt=[DateTimeOffset]::Now.ToString('o')} |
        ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $validationRoot 'status.json') -Encoding utf8
}
function Invoke-Check([string]$Name, [string]$Executable, [string[]]$Arguments) {
    Set-Stage $Name @{arguments=$Arguments}
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WorkingDirectory (Get-Location).Path -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $validationRoot "$Name.stdout.log") -RedirectStandardError (Join-Path $validationRoot "$Name.stderr.log")
    $null = $process.Handle
    $process.WaitForExit()
    $process.Refresh()
    $checks[$Name] = @{exitCode=$process.ExitCode}
    if ($null -eq $process.ExitCode -or $process.ExitCode -ne 0) { throw "Check failed: $Name, exit=$($process.ExitCode)" }
}
try {
    Import-Module 'D:\DevTools\VSBuildTools\Common7\Tools\Microsoft.VisualStudio.DevShell.dll'
    Enter-VsDevShell -VsInstallPath 'D:\DevTools\VSBuildTools' -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
    $msvcLinker = (Get-Command link.exe -ErrorAction Stop).Source
    if ($msvcLinker -notlike 'D:\DevTools\VSBuildTools\*') { throw "MSVC linker was not selected: $msvcLinker" }
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $msvcLinker
    $env:PATH = 'D:\Git\bin;D:\DevTools\Rust\cargo\bin;E:\Tools\nodejs;' + $env:PATH
    $env:CARGO_TARGET_DIR = 'D:\github\CodexPlusPlus-compressed-session-fix-20261015\target'
    $env:CODEX_GATE_TEST_POWERSHELL = (Get-Command pwsh.exe).Source
    Set-Location -LiteralPath $SourceRoot
    if ((git rev-parse HEAD) -ne $ExpectedCommit) { throw 'Wrong source commit' }
    git diff --quiet HEAD
    $package = Get-Content -LiteralPath 'apps\codex-plus-manager\package.json' -Raw | ConvertFrom-Json
    $lock = Get-Content -LiteralPath 'apps\codex-plus-manager\package-lock.json' -Raw | ConvertFrom-Json -AsHashtable
    $tauri = Get-Content -LiteralPath 'apps\codex-plus-manager\src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json
    if ($package.version -ne $Version -or $lock.version -ne $Version -or $lock.packages[''].version -ne $Version -or $tauri.version -ne $Version) { throw 'Manifest version mismatch' }
    @{source=$ExpectedCommit; linker=$msvcLinker; rustc=(& rustc --version); node=(& node --version); target=$env:CARGO_TARGET_DIR} |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $validationRoot 'toolchain.json') -Encoding utf8
    $cargoExe = (Get-Command cargo.exe).Source
    Invoke-Check 'core-tests' $cargoExe @('test','--locked','-p','codex-plus-core','--lib','--test','relay_switch','--test','relay_config','--test','protocol_proxy','--test','resident_helper','--test','helper_body_limit_settings','--test','proxy_auto_retry_settings','--test','issue_20261018_compat')
    Invoke-Check 'data-tests' $cargoExe @('test','--locked','-p','codex-plus-data')
    Invoke-Check 'launcher-tests' $cargoExe @('test','--locked','-p','codex-plus-launcher')
    Set-Location -LiteralPath (Join-Path $SourceRoot 'apps\codex-plus-manager')
    $npmExe = (Get-Command npm.cmd).Source
    Invoke-Check 'frontend-install' $npmExe @('ci','--ignore-scripts','--no-audit','--no-fund')
    Invoke-Check 'frontend-check' $npmExe @('run','check')
    Invoke-Check 'frontend-tests' $npmExe @('test')
    Invoke-Check 'frontend-build' $npmExe @('run','vite:build')
    Set-Location -LiteralPath $SourceRoot
    Invoke-Check 'release-build' $cargoExe @('build','--release','--locked','-p','codex-plus-manager','--bin','codex-plus-plus')
    Invoke-Check 'probe-build' $cargoExe @('build','--release','--locked','-p','codex-plus-data','--example','provider_sync_probe')
    git diff --quiet HEAD
    $outputRoot = Join-Path $SourceRoot "dist\jacy-build\$Version"
    New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
    $manager = Join-Path $outputRoot 'codex-plus-plus.exe'
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release\codex-plus-plus.exe') -Destination $manager
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release\examples\provider_sync_probe.exe') -Destination (Join-Path $outputRoot 'provider_sync_probe.exe')
    $pe = (Get-Item -LiteralPath $manager).VersionInfo
    if ($pe.FileVersion -ne $Version -or $pe.ProductVersion -ne $Version) { throw 'Windows binary version mismatch' }
    $archive = Join-Path $outputRoot "CodexPlusPlus-$Version-windows-x64.zip"
    Compress-Archive -LiteralPath $manager,(Join-Path $outputRoot 'provider_sync_probe.exe') -DestinationPath $archive -CompressionLevel Optimal -Force
    Set-Stage 'complete' @{fileVersion=$pe.FileVersion; productVersion=$pe.ProductVersion; hashes=@(Get-FileHash -Algorithm SHA256 -LiteralPath $manager,(Join-Path $outputRoot 'provider_sync_probe.exe'),$archive | Select-Object Path,Hash)}
} catch {
    Set-Stage 'failed' @{error=$_.Exception.Message}
    throw
}
