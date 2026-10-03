[CmdletBinding()]
param([Parameter(Mandatory)][string]$ExpectedAppDirectory)
$ErrorActionPreference = 'Stop'
$logPath = Join-Path $PSScriptRoot 'codex_launch_readiness.log'
try {
    . (Join-Path $PSScriptRoot 'CodexRuntimeReadiness.ps1')
    function Assert-ExpectedPackage {
        $package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
        if (-not $package) { throw 'Installed Codex package is missing.' }
        $installedApp = Join-Path $package.InstallLocation 'app'
        if (-not [string]::Equals([IO.Path]::GetFullPath($ExpectedAppDirectory).TrimEnd('\'), [IO.Path]::GetFullPath($installedApp).TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) {
            throw 'The selected application is not the current Codex package. Retry the launch.'
        }
        return $package
    }
    $package = Assert-ExpectedPackage
    $ready = Get-CodexRuntimeReadiness -UseReceipt
    if (-not $ready.Ready) {
        # 只运行修复，不启动应用，避免启动检查递归。
        $start = [Diagnostics.ProcessStartInfo]::new()
        $start.FileName = (Get-Process -Id $PID).Path
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        foreach ($argument in @('-NoLogo','-NoProfile','-NonInteractive','-File',(Join-Path $PSScriptRoot 'Repair-CodexAfterPackageUpdate.ps1'),'-IfNeeded','-MaximumAttempts','1','-Quiet')) {
            [void]$start.ArgumentList.Add($argument)
        }
        $repair = [Diagnostics.Process]::Start($start)
        try {
            if (-not $repair.WaitForExit(240000)) {
                $repair.Kill($true)
                $repair.WaitForExit()
                throw 'Runtime repair timed out; application launch was blocked.'
            }
            if ($repair.ExitCode -ne 0) { throw "Runtime repair failed with exit code $($repair.ExitCode)." }
        } finally { $repair.Dispose() }
        $ready = Get-CodexRuntimeReadiness -UseReceipt
    }
    if (-not $ready.Ready) { throw "Runtime verification failed: $($ready.Error)" }
    $current = Assert-ExpectedPackage
    if ($package.PackageFullName -ne $current.PackageFullName) { throw 'Package changed during the check. Retry the launch.' }
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'NodeReplProxyLauncher.disabled'))) {
        $proxy = [Net.Sockets.TcpClient]::new()
        try {
            $connect = $proxy.ConnectAsync('127.0.0.1',7897)
            if (-not $connect.Wait(2000) -or -not $proxy.Connected) { throw 'The configured local browser proxy is unavailable.' }
        } finally { $proxy.Dispose() }
    }
    Add-Content -LiteralPath $logPath -Value "[$((Get-Date).ToString('o'))] passed package=$($current.Version) receiptReused=$($ready.ReceiptReused)" -Encoding utf8
    exit 0
} catch {
    Add-Content -LiteralPath $logPath -Value "[$((Get-Date).ToString('o'))] blocked reason=$($_.Exception.Message)" -Encoding utf8
    exit 1
}
