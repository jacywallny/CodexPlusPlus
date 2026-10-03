[CmdletBinding()]
param([Parameter(Mandatory)][string]$LauncherPath)
$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot ('launcher-fixtures\' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$testLauncher = Join-Path $root 'codex-plus-plus.exe'
Copy-Item -LiteralPath $LauncherPath -Destination $testLauncher
$pwshPath = (Get-Command pwsh.exe -ErrorAction Stop).Source
$scriptPath = Join-Path $root '检查 with spaces.ps1'
$configPath = Join-Path $root 'codex-plus-plus.runtime-gate.json'
$app = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
$appDirectory = Join-Path $app.InstallLocation 'app'

function Invoke-Check {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $testLauncher
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardError = $true
    foreach ($arg in @('--check-runtime-only','--app-path',$appDirectory)) { [void]$start.ArgumentList.Add($arg) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        if (-not $process.WaitForExit(20000)) { $process.Kill($true); throw 'Binary check exceeded 20 seconds.' }
        return $process.ExitCode
    } finally { $process.Dispose() }
}

$before = @(Get-CimInstance Win32_Process -Filter "Name='ChatGPT.exe'" | Select-Object -ExpandProperty ProcessId)
$config = @{schema_version=1;powershell=$pwshPath;script=$scriptPath}
$config | ConvertTo-Json | Set-Content -LiteralPath $configPath -Encoding utf8NoBOM
Set-Content -LiteralPath $scriptPath -Value "param([string]`$ExpectedAppDirectory)`nexit 7" -Encoding utf8NoBOM
if ((Invoke-Check) -eq 0) { throw 'A failed runtime check was not rejected.' }
Set-Content -LiteralPath $scriptPath -Value "param([string]`$ExpectedAppDirectory)`n[IO.File]::WriteAllText((Join-Path `$PSScriptRoot 'received.txt'), `$ExpectedAppDirectory)`nexit 0" -Encoding utf8NoBOM
if ((Invoke-Check) -ne 0) { throw 'A successful runtime check was rejected.' }
if ((Get-Content -LiteralPath (Join-Path $root 'received.txt') -Raw) -ne $appDirectory) { throw 'The application directory was changed by argument parsing.' }
Set-Content -LiteralPath $configPath -Value '{' -Encoding utf8NoBOM
if ((Invoke-Check) -eq 0) { throw 'Malformed configuration was not rejected.' }
$after = @(Get-CimInstance Win32_Process -Filter "Name='ChatGPT.exe'" | Select-Object -ExpandProperty ProcessId)
if (@(Compare-Object $before $after).Count -ne 0) { throw 'Runtime-only checks changed Codex desktop processes.' }
[pscustomobject]@{Passed=3;Cases=@('failed check blocks','valid check accepts exact application argument','malformed config blocks');DesktopProcessesUnchanged=$true;FixtureRoot=$root} | ConvertTo-Json
