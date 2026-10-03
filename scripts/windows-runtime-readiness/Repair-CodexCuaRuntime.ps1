[CmdletBinding()]
param(
    [string]$LogPath = 'D:\CodexContextMenu\codex_cua_runtime_repair.log',
    [string]$ProxyLauncherPath = 'D:\CodexContextMenu\NodeReplProxyLauncher\NodeReplProxyLauncher.exe',
    [string]$ExpectedProxyLauncherSha256 = 'eecab71f8fe2a26d8793c086e0a5dcc89d1622b542132c16dd8490ec09829a5b',
    [long]$ExpectedProxyLauncherLength = 3143680,
    [string]$ProxyDisableMarker = 'D:\CodexContextMenu\NodeReplProxyLauncher.disabled',
    [string]$ProxyUrl = 'http://127.0.0.1:7897',
    [string]$NoProxy = 'localhost,127.0.0.1,::1',
    [string]$StableStoreRoot = 'D:\CodexContextMenu\CuaRuntimeStore',
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
$Utf8NoBom = [System.Text.UTF8Encoding]::new($false)

function Write-RepairLog {
    param([Parameter(Mandatory)][string]$Message)

    $line = '[{0}] {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
    Add-Content -LiteralPath $LogPath -Value $line -Encoding UTF8
    if (-not $Quiet) {
        Write-Host $line
    }
}

function Get-CurrentCodexPackage {
    Get-AppxPackage -Name 'OpenAI.Codex' |
        Sort-Object Version -Descending |
        Select-Object -First 1
}

function Get-StringSha256 {
    param([Parameter(Mandatory)][string]$Value)

    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Value)
        return ([Convert]::ToHexString($sha256.ComputeHash($bytes))).ToLowerInvariant()
    } finally {
        $sha256.Dispose()
    }
}

function Copy-FileWithoutEfs {
    param(
        [Parameter(Mandatory)][string]$Source,
        [Parameter(Mandatory)][string]$Destination
    )

    $destinationDirectory = Split-Path -Parent $Destination
    [System.IO.Directory]::CreateDirectory($destinationDirectory) | Out-Null

    $sourceStream = [System.IO.File]::Open(
        $Source,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::ReadWrite
    )
    try {
        $destinationStream = [System.IO.File]::Open(
            $Destination,
            [System.IO.FileMode]::Create,
            [System.IO.FileAccess]::Write,
            [System.IO.FileShare]::None
        )
        try {
            $sourceStream.CopyTo($destinationStream)
            $destinationStream.Flush($true)
        } finally {
            $destinationStream.Dispose()
        }
    } finally {
        $sourceStream.Dispose()
    }

    $sourceItem = Get-Item -LiteralPath $Source -Force
    [System.IO.File]::SetLastWriteTimeUtc($Destination, $sourceItem.LastWriteTimeUtc)
}

function Get-CuaSourceFingerprint {
    param(
        [Parameter(Mandatory)][string]$SourceRoot,
        [Parameter(Mandatory)][string]$PackageVersion
    )

    $manifestPath = Join-Path $SourceRoot 'manifest.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "Bundled cua_node manifest is missing: $manifestPath"
    }

    $coreRelativePaths = @(
        'bin\node.exe',
        'bin\node_repl.exe',
        'bin\node_modules\@oai\sky\package.json',
        'bin\node_modules\@oai\sky\bin\windows\codex-computer-use.exe',
        'bin\node_modules\@oai\sky\dist\project\cua\sky_js\src\targets\windows\internal\helper_transport.js'
    )
    $coreFiles = [ordered]@{}
    foreach ($relativePath in $coreRelativePaths) {
        $sourcePath = Join-Path $SourceRoot $relativePath
        if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
            throw "Bundled cua_node core file is missing: $sourcePath"
        }
        $sourceItem = Get-Item -LiteralPath $sourcePath -Force
        $coreFiles[$relativePath] = [ordered]@{
            length = $sourceItem.Length
            sha256 = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }

    $manifestHash = (
        Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256
    ).Hash.ToLowerInvariant()
    $fingerprintLines = [System.Collections.Generic.List[string]]::new()
    $fingerprintLines.Add("manifest=$manifestHash")
    foreach ($entry in $coreFiles.GetEnumerator()) {
        $fingerprintLines.Add(
            ('{0}|{1}|{2}' -f $entry.Key, $entry.Value.length, $entry.Value.sha256)
        )
    }
    $fingerprintHash = Get-StringSha256 -Value ($fingerprintLines -join "`n")

    return [pscustomobject]@{
        packageVersion = $PackageVersion
        manifestHash = $manifestHash
        fingerprintHash = $fingerprintHash
        runtimeId = $fingerprintHash.Substring(0, 16)
        coreFiles = $coreFiles
    }
}

function Get-NodeReplProxySpec {
    param([Parameter(Mandatory)][pscustomobject]$Fingerprint)

    if (Test-Path -LiteralPath $ProxyDisableMarker -PathType Leaf) {
        return [pscustomobject]@{ enabled = $false }
    }
    if (-not (Test-Path -LiteralPath $ProxyLauncherPath -PathType Leaf)) {
        throw "Node REPL proxy launcher is missing: $ProxyLauncherPath"
    }

    $launcherItem = Get-Item -LiteralPath $ProxyLauncherPath -Force
    $launcherHash = (
        Get-FileHash -LiteralPath $ProxyLauncherPath -Algorithm SHA256
    ).Hash.ToLowerInvariant()
    if (
        $launcherItem.Length -ne $ExpectedProxyLauncherLength -or
        $launcherHash -ne $ExpectedProxyLauncherSha256
    ) {
        throw (
            "Node REPL proxy launcher failed its fixed release gate: " +
            "length=$($launcherItem.Length), sha256=$launcherHash"
        )
    }
    $officialNodeRepl = $Fingerprint.coreFiles['bin\node_repl.exe']
    $config = [ordered]@{
        schemaVersion = 1
        officialExecutable = 'node_repl.official.exe'
        officialSha256 = $officialNodeRepl.sha256
        httpProxy = $ProxyUrl
        httpsProxy = $ProxyUrl
        noProxy = $NoProxy
        nodeUseEnvProxy = '1'
        logPath = 'D:\CodexContextMenu\node_repl_proxy_launcher.log'
        disableMarker = $ProxyDisableMarker
    }
    $configText = ($config | ConvertTo-Json) + [Environment]::NewLine
    $configHash = Get-StringSha256 -Value $configText
    $identity = @(
        'kind=codex-cua-node-repl-proxy'
        'schema=1'
        "sourceFingerprintSha256=$($Fingerprint.fingerprintHash)"
        "sourceNodeReplSha256=$($officialNodeRepl.sha256)"
        "launcherSha256=$launcherHash"
        "configSha256=$configHash"
        'companion=bin\node_repl.official.exe'
    ) -join "`n"
    $runtimeFingerprint = Get-StringSha256 -Value $identity
    return [pscustomobject]@{
        enabled = $true
        launcherPath = $ProxyLauncherPath
        launcherLength = $launcherItem.Length
        launcherSha256 = $launcherHash
        configText = $configText
        configSha256 = $configHash
        officialRelativePath = 'bin\node_repl.official.exe'
        officialLength = $officialNodeRepl.length
        officialSha256 = $officialNodeRepl.sha256
        runtimeFingerprintSha256 = $runtimeFingerprint
        runtimeId = $runtimeFingerprint.Substring(0, 16)
    }
}

function Install-NodeReplProxyLauncher {
    param(
        [Parameter(Mandatory)][string]$RuntimeRoot,
        [Parameter(Mandatory)][pscustomobject]$ProxySpec
    )

    if (-not $ProxySpec.enabled) {
        return
    }
    $nodeReplPath = Join-Path $RuntimeRoot 'bin\node_repl.exe'
    $officialPath = Join-Path $RuntimeRoot $ProxySpec.officialRelativePath
    $configPath = Join-Path $RuntimeRoot 'bin\node_repl.proxy.json'
    Move-Item -LiteralPath $nodeReplPath -Destination $officialPath
    Copy-FileWithoutEfs -Source $ProxySpec.launcherPath -Destination $nodeReplPath
    [System.IO.File]::WriteAllText($configPath, $ProxySpec.configText, $Utf8NoBom)
}

function Test-CuaRuntime {
    param(
        [Parameter(Mandatory)][string]$CandidateRoot,
        [Parameter(Mandatory)][pscustomobject]$Fingerprint,
        [Parameter(Mandatory)][pscustomobject]$ProxySpec
    )

    $candidateManifestPath = Join-Path $CandidateRoot 'manifest.json'
    if (-not (Test-Path -LiteralPath $candidateManifestPath -PathType Leaf)) {
        return $false
    }
    if (
        (Get-FileHash -LiteralPath $candidateManifestPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne
        $Fingerprint.manifestHash
    ) {
        return $false
    }

    foreach ($entry in $Fingerprint.coreFiles.GetEnumerator()) {
        $candidateRelativePath = if (
            $ProxySpec.enabled -and $entry.Key -eq 'bin\node_repl.exe'
        ) {
            $ProxySpec.officialRelativePath
        } else {
            $entry.Key
        }
        $candidatePath = Join-Path $CandidateRoot $candidateRelativePath
        if (-not (Test-Path -LiteralPath $candidatePath -PathType Leaf)) {
            return $false
        }
        $candidateItem = Get-Item -LiteralPath $candidatePath -Force
        if ($candidateItem.Attributes -band [System.IO.FileAttributes]::Encrypted) {
            return $false
        }
        if ($candidateItem.Length -ne $entry.Value.length) {
            return $false
        }
        if (
            (Get-FileHash -LiteralPath $candidatePath -Algorithm SHA256).Hash.ToLowerInvariant() -ne
            $entry.Value.sha256
        ) {
            return $false
        }
    }

    if ($ProxySpec.enabled) {
        $launcherPath = Join-Path $CandidateRoot 'bin\node_repl.exe'
        $configPath = Join-Path $CandidateRoot 'bin\node_repl.proxy.json'
        if (
            -not (Test-Path -LiteralPath $launcherPath -PathType Leaf) -or
            -not (Test-Path -LiteralPath $configPath -PathType Leaf)
        ) {
            return $false
        }
        $launcherItem = Get-Item -LiteralPath $launcherPath -Force
        if (
            ($launcherItem.Attributes -band [System.IO.FileAttributes]::Encrypted) -or
            $launcherItem.Length -ne $ProxySpec.launcherLength -or
            (Get-FileHash -LiteralPath $launcherPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne
                $ProxySpec.launcherSha256 -or
            (Get-StringSha256 -Value (Get-Content -LiteralPath $configPath -Raw)) -ne
                $ProxySpec.configSha256
        ) {
            return $false
        }
    }

    $markerPath = Join-Path $CandidateRoot '.codex-runtime-sync.json'
    if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) {
        return $false
    }
    try {
        $marker = Get-Content -LiteralPath $markerPath -Raw | ConvertFrom-Json
        $baseMatches = (
            $marker.schemaVersion -eq $(if ($ProxySpec.enabled) { 2 } else { 1 }) -and
            $marker.sourceManifestSha256 -eq $Fingerprint.manifestHash -and
            $marker.sourceFingerprintSha256 -eq $Fingerprint.fingerprintHash
        )
        if (-not $baseMatches) {
            return $false
        }
        if (-not $ProxySpec.enabled) {
            return $true
        }
        return (
            $marker.proxyLauncher.enabled -eq $true -and
            $marker.runtimeKind -eq 'cua-node-repl-proxy' -and
            $marker.runtimeId -eq $ProxySpec.runtimeId -and
            $marker.runtimeFingerprintSha256 -eq $ProxySpec.runtimeFingerprintSha256 -and
            $marker.proxyLauncher.launcherSha256 -eq $ProxySpec.launcherSha256 -and
            $marker.proxyLauncher.configSha256 -eq $ProxySpec.configSha256 -and
            $marker.proxyLauncher.officialNodeReplSha256 -eq $ProxySpec.officialSha256
        )
    } catch {
        return $false
    }
}

function Ensure-CuaRuntime {
    param(
        [Parameter(Mandatory)][string]$AppxSourceRoot,
        [Parameter(Mandatory)][string]$CopySourceRoot,
        [Parameter(Mandatory)][string]$TargetRoot,
        [Parameter(Mandatory)][string]$RuntimeId,
        [Parameter(Mandatory)][pscustomobject]$Fingerprint,
        [Parameter(Mandatory)][pscustomobject]$ProxySpec,
        [Parameter(Mandatory)][string]$PackageVersion
    )

    if (
        (Test-Path -LiteralPath $TargetRoot -PathType Container) -and
        (Test-CuaRuntime -CandidateRoot $TargetRoot -Fingerprint $Fingerprint -ProxySpec $ProxySpec)
    ) {
        Write-RepairLog (
            "repair-skipped runtimeId=$RuntimeId reason=verified-current " +
            "packageVersion=$PackageVersion"
        )
        return
    }

    $repairStartedAt = Get-Date
    $targetParent = Split-Path -Parent $TargetRoot
    $stagingPrefix = '.staging-{0}-' -f $RuntimeId
    $stagingCandidates = @(
        Get-ChildItem -LiteralPath $targetParent -Directory -Force -ErrorAction SilentlyContinue |
            Where-Object {
                $_.Name.StartsWith(
                    $stagingPrefix,
                    [System.StringComparison]::OrdinalIgnoreCase
                )
            } |
            Sort-Object LastWriteTime -Descending
    )
    $stagingReused = $stagingCandidates.Count -gt 0
    $stagingRoot = if ($stagingReused) {
        $stagingCandidates[0].FullName
    } else {
        Join-Path $targetParent (
            '{0}{1}' -f $stagingPrefix, [guid]::NewGuid().ToString('N')
        )
    }
    $resolvedTargetParent = [System.IO.Path]::GetFullPath($targetParent).TrimEnd('\')
    $resolvedStagingRoot = [System.IO.Path]::GetFullPath($stagingRoot)
    if (
        -not [string]::Equals(
            [System.IO.Path]::GetDirectoryName($resolvedStagingRoot),
            $resolvedTargetParent,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -or
        -not [System.IO.Path]::GetFileName($resolvedStagingRoot).StartsWith(
            $stagingPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )
    ) {
        throw "Refusing to use an unexpected CUA staging path: $resolvedStagingRoot"
    }
    [System.IO.Directory]::CreateDirectory($resolvedStagingRoot) | Out-Null
    $stagingRoot = $resolvedStagingRoot
    Write-RepairLog (
        "repair-start packageVersion=$PackageVersion runtimeId=$RuntimeId " +
        "source=$CopySourceRoot staging=$stagingRoot reused=$stagingReused"
    )

    try {
        $sourceFiles = @(Get-ChildItem -LiteralPath $AppxSourceRoot -Recurse -File -Force)
        $sourceBytes = ($sourceFiles | Measure-Object Length -Sum).Sum
        $sourceRelativePaths = [System.Collections.Generic.HashSet[string]]::new(
            [System.StringComparer]::OrdinalIgnoreCase
        )
        foreach ($sourceFile in $sourceFiles) {
            [void]$sourceRelativePaths.Add(
                [System.IO.Path]::GetRelativePath($AppxSourceRoot, $sourceFile.FullName)
            )
        }
        if ($stagingReused) {
            foreach ($stagedFile in @(
                Get-ChildItem -LiteralPath $stagingRoot -Recurse -File -Force -ErrorAction SilentlyContinue
            )) {
                $stagedRelativePath = [System.IO.Path]::GetRelativePath(
                    $stagingRoot,
                    $stagedFile.FullName
                )
                if (-not $sourceRelativePaths.Contains($stagedRelativePath)) {
                    Remove-Item -LiteralPath $stagedFile.FullName -Force
                }
            }
        }

        $copied = 0
        $reused = 0
        $processed = 0
        foreach ($sourceFile in $sourceFiles) {
            $relativePath = [System.IO.Path]::GetRelativePath($AppxSourceRoot, $sourceFile.FullName)
            $copySourcePath = Join-Path $CopySourceRoot $relativePath
            if (-not (Test-Path -LiteralPath $copySourcePath -PathType Leaf)) {
                throw "CUA copy source is missing: $copySourcePath"
            }
            $stagedPath = Join-Path $stagingRoot $relativePath
            $stagedItem = Get-Item -LiteralPath $stagedPath -Force -ErrorAction SilentlyContinue
            $canReuse = (
                $stagedItem -and
                -not ($stagedItem.Attributes -band [System.IO.FileAttributes]::Encrypted) -and
                $stagedItem.Length -eq (Get-Item -LiteralPath $copySourcePath -Force).Length
            )
            if ($canReuse) {
                $sourceHash = (
                    Get-FileHash -LiteralPath $copySourcePath -Algorithm SHA256
                ).Hash
                $stagedHash = (
                    Get-FileHash -LiteralPath $stagedPath -Algorithm SHA256
                ).Hash
                $canReuse = $sourceHash -eq $stagedHash
            }
            if ($canReuse) {
                $reused++
            } else {
                Copy-FileWithoutEfs -Source $copySourcePath -Destination $stagedPath
                $copied++
            }
            $processed++
            if ($processed % 500 -eq 0) {
                Write-RepairLog (
                    "copy-progress runtimeId=$RuntimeId processed=$processed " +
                    "copied=$copied reused=$reused total=$($sourceFiles.Count)"
                )
            }
        }
        Write-RepairLog (
            "copy-complete runtimeId=$RuntimeId copied=$copied reused=$reused " +
            "total=$($sourceFiles.Count)"
        )

        $stagedFiles = @(Get-ChildItem -LiteralPath $stagingRoot -Recurse -File -Force)
        $stagedBytes = ($stagedFiles | Measure-Object Length -Sum).Sum
        if ($stagedFiles.Count -ne $sourceFiles.Count -or $stagedBytes -ne $sourceBytes) {
            throw (
                "CUA copy shape mismatch: sourceFiles=$($sourceFiles.Count), " +
                "stagedFiles=$($stagedFiles.Count), sourceBytes=$sourceBytes, stagedBytes=$stagedBytes"
            )
        }

        foreach ($sourceFile in $sourceFiles) {
            $relativePath = [System.IO.Path]::GetRelativePath($AppxSourceRoot, $sourceFile.FullName)
            $stagedPath = Join-Path $stagingRoot $relativePath
            $stagedItem = Get-Item -LiteralPath $stagedPath -Force
            if ($stagedItem.Attributes -band [System.IO.FileAttributes]::Encrypted) {
                throw "CUA staged file retained AppX encryption: $relativePath"
            }
            if ($stagedItem.Length -ne $sourceFile.Length) {
                throw "CUA staged file length mismatch: $relativePath"
            }
            $sourceHash = (Get-FileHash -LiteralPath $sourceFile.FullName -Algorithm SHA256).Hash
            $stagedHash = (Get-FileHash -LiteralPath $stagedPath -Algorithm SHA256).Hash
            if ($stagedHash -ne $sourceHash) {
                throw "CUA staged file hash mismatch: $relativePath"
            }
        }

        Install-NodeReplProxyLauncher -RuntimeRoot $stagingRoot -ProxySpec $ProxySpec

        $marker = [ordered]@{
            schemaVersion = $(if ($ProxySpec.enabled) { 2 } else { 1 })
            runtimeKind = $(if ($ProxySpec.enabled) { 'cua-node-repl-proxy' } else { 'cua-node-canonical' })
            runtimeId = $RuntimeId
            packageVersion = $PackageVersion
            sourceManifestSha256 = $Fingerprint.manifestHash
            sourceFingerprintSha256 = $Fingerprint.fingerprintHash
            sourceFileCount = $sourceFiles.Count
            sourceBytes = $sourceBytes
            createdAt = (Get-Date).ToString('o')
            coreFiles = $Fingerprint.coreFiles
        }
        if ($ProxySpec.enabled) {
            $marker['runtimeFingerprintSha256'] = $ProxySpec.runtimeFingerprintSha256
            $marker['proxyLauncher'] = [ordered]@{
                enabled = $true
                launcherRelativePath = 'bin\node_repl.exe'
                launcherLength = $ProxySpec.launcherLength
                launcherSha256 = $ProxySpec.launcherSha256
                configRelativePath = 'bin\node_repl.proxy.json'
                configLength = $ProxySpec.configText.Length
                configSha256 = $ProxySpec.configSha256
                officialNodeReplRelativePath = $ProxySpec.officialRelativePath
                officialNodeReplLength = $ProxySpec.officialLength
                officialNodeReplSha256 = $ProxySpec.officialSha256
                proxy = $ProxyUrl
                disableMarker = $ProxyDisableMarker
            }
        }
        [System.IO.File]::WriteAllText(
            (Join-Path $stagingRoot '.codex-runtime-sync.json'),
            (($marker | ConvertTo-Json -Depth 10) + [Environment]::NewLine),
            $Utf8NoBom
        )
        if (-not (Test-CuaRuntime -CandidateRoot $stagingRoot -Fingerprint $Fingerprint -ProxySpec $ProxySpec)) {
            throw 'CUA staged runtime did not pass the final core verification.'
        }

        if (Test-Path -LiteralPath $TargetRoot) {
            $invalidRoot = Join-Path $targetParent (
                '.invalid-{0}-{1}-{2}' -f
                    $RuntimeId,
                    (Get-Date -Format 'yyyyMMdd-HHmmss'),
                    [guid]::NewGuid().ToString('N').Substring(0, 8)
            )
            Move-Item -LiteralPath $TargetRoot -Destination $invalidRoot
            Write-RepairLog "invalid-runtime-preserved path=$invalidRoot"
        }
        Move-Item -LiteralPath $stagingRoot -Destination $TargetRoot
        if (-not (Test-CuaRuntime -CandidateRoot $TargetRoot -Fingerprint $Fingerprint -ProxySpec $ProxySpec)) {
            throw "Installed CUA runtime failed verification: $TargetRoot"
        }

        $durationMs = [math]::Round(((Get-Date) - $repairStartedAt).TotalMilliseconds)
        Write-RepairLog (
            "repair-ok runtimeId=$RuntimeId files=$($sourceFiles.Count) " +
            "bytes=$sourceBytes durationMs=$durationMs"
        )
    } catch {
        Write-RepairLog (
            "repair-failed runtimeId=$RuntimeId error=$($_.Exception.Message) " +
            "staging=$stagingRoot"
        )
        throw
    }
}

function Register-StableCuaProxyRuntime {
    param(
        [Parameter(Mandatory)][string]$RuntimeRoot,
        [Parameter(Mandatory)][string]$StableRoot,
        [Parameter(Mandatory)][string]$RuntimeId,
        [Parameter(Mandatory)]$Fingerprint,
        [Parameter(Mandatory)]$ProxySpec
    )
    if ($RuntimeId -notmatch '^[0-9a-f]{16}$') { throw 'Invalid stable CUA runtime ID.' }
    if (-not (Test-CuaRuntime -CandidateRoot $StableRoot -Fingerprint $Fingerprint -ProxySpec $ProxySpec)) {
        throw 'Stable CUA proxy verification failed before registration.'
    }
    $linkPath = Join-Path $RuntimeRoot $RuntimeId
    if ([IO.Path]::GetFullPath((Split-Path -Parent $linkPath)) -ne [IO.Path]::GetFullPath($RuntimeRoot).TrimEnd('\')) {
        throw 'Managed CUA link escaped its root.'
    }
    $existing = Get-Item -LiteralPath $linkPath -Force -ErrorAction SilentlyContinue
    if ($existing -and $existing.LinkType -eq 'Junction') {
        if (-not [string]::Equals([IO.Path]::GetFullPath([string]($existing.Target | Select-Object -First 1)), [IO.Path]::GetFullPath($StableRoot), [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Existing CUA proxy junction points to another store.'
        }
        Write-RepairLog "stable-proxy-link-reused runtimeId=$RuntimeId target=$StableRoot"
        return
    }
    $backupPath = $null
    if ($existing) {
        $active = Get-Item -LiteralPath 'D:\CodexContextMenu\CodexBundledResources\cua_node' -Force -ErrorAction SilentlyContinue
        $activeTarget = [string]($active.Target | Select-Object -First 1)
        $package = Get-CurrentCodexPackage
        $appProcesses = @(Get-CimInstance Win32_Process -Filter "Name='ChatGPT.exe'" | Where-Object {
            $_.ExecutablePath -and $_.ExecutablePath.StartsWith($package.InstallLocation + '\', [StringComparison]::OrdinalIgnoreCase)
        })
        if ($appProcesses.Count -and [string]::Equals($activeTarget, $linkPath, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'The legacy proxy runtime is active. Close Codex before migration.'
        }
        $inUse = @(Get-CimInstance Win32_Process | Where-Object {
            $_.ExecutablePath -and $_.ExecutablePath.StartsWith($linkPath + '\', [StringComparison]::OrdinalIgnoreCase)
        })
        if ($inUse.Count) { throw 'The legacy proxy runtime is in use.' }
        $marker = Get-Content -LiteralPath (Join-Path $linkPath '.codex-runtime-sync.json') -Raw | ConvertFrom-Json
        if ($marker.runtimeId -ne $RuntimeId -or $marker.runtimeKind -ne 'cua-node-repl-proxy') { throw 'Unexpected legacy runtime; no files moved.' }
        $backupPath = Join-Path $RuntimeRoot ($RuntimeId + '.backup-stable-proxy-' + [guid]::NewGuid().ToString('N'))
        if ([IO.Path]::GetFullPath((Split-Path -Parent $backupPath)) -ne [IO.Path]::GetFullPath($RuntimeRoot).TrimEnd('\')) { throw 'Backup escaped its root.' }
        Move-Item -LiteralPath $linkPath -Destination $backupPath
    }
    try { New-Item -ItemType Junction -Path $linkPath -Target $StableRoot | Out-Null }
    catch {
        if ($backupPath -and -not (Test-Path -LiteralPath $linkPath)) { Move-Item -LiteralPath $backupPath -Destination $linkPath }
        throw
    }
    Write-RepairLog "stable-proxy-link-registered runtimeId=$RuntimeId target=$StableRoot backup=$backupPath"
}

$runtimeMutex = [Threading.Mutex]::new($false, 'Local\CodexCuaRuntimeMaterialization')
$runtimeMutexHeld = $false
try {
    try {
        $runtimeMutexHeld = $runtimeMutex.WaitOne([TimeSpan]::FromMinutes(5))
    } catch [Threading.AbandonedMutexException] {
        $runtimeMutexHeld = $true
    }
    if (-not $runtimeMutexHeld) {
        throw 'Timed out waiting for the CUA runtime materialization lock.'
    }

    $package = Get-CurrentCodexPackage
    if (-not $package) {
        throw 'OpenAI.Codex AppX package was not found.'
    }
    $sourceRoot = Join-Path $package.InstallLocation 'app\resources\cua_node'
    if (-not (Test-Path -LiteralPath $sourceRoot -PathType Container)) {
        throw "Bundled cua_node source is missing: $sourceRoot"
    }

    $runtimeRoot = Join-Path $env:LOCALAPPDATA 'OpenAI\Codex\runtimes\cua_node'
    [System.IO.Directory]::CreateDirectory($runtimeRoot) | Out-Null
    $fingerprint = Get-CuaSourceFingerprint `
        -SourceRoot $sourceRoot `
        -PackageVersion ([string]$package.Version)

    $canonicalSpec = [pscustomobject]@{ enabled = $false }
    $canonicalRoot = Join-Path $runtimeRoot $fingerprint.runtimeId
    Ensure-CuaRuntime `
        -AppxSourceRoot $sourceRoot `
        -CopySourceRoot $sourceRoot `
        -TargetRoot $canonicalRoot `
        -RuntimeId $fingerprint.runtimeId `
        -Fingerprint $fingerprint `
        -ProxySpec $canonicalSpec `
        -PackageVersion ([string]$package.Version)

    $proxySpec = Get-NodeReplProxySpec -Fingerprint $fingerprint
    if (-not $proxySpec.enabled) {
        Write-RepairLog "proxy-mode-disabled marker=$ProxyDisableMarker canonical=$canonicalRoot"
        return
    }

    # 在 App 清理目录之外构建。重启中的 App 不能删除尚未完成的代理运行时。
    [IO.Directory]::CreateDirectory($StableStoreRoot) | Out-Null
    $proxyRoot = Join-Path $StableStoreRoot $proxySpec.runtimeId
    Ensure-CuaRuntime `
        -AppxSourceRoot $sourceRoot `
        -CopySourceRoot $sourceRoot `
        -TargetRoot $proxyRoot `
        -RuntimeId $proxySpec.runtimeId `
        -Fingerprint $fingerprint `
        -ProxySpec $proxySpec `
        -PackageVersion ([string]$package.Version)
    $currentPackage = Get-CurrentCodexPackage
    if ($currentPackage.PackageFullName -ne $package.PackageFullName) { throw 'Codex package changed during proxy preparation.' }
    Register-StableCuaProxyRuntime -RuntimeRoot $runtimeRoot -StableRoot $proxyRoot `
        -RuntimeId $proxySpec.runtimeId -Fingerprint $fingerprint -ProxySpec $proxySpec
} finally {
    if ($runtimeMutexHeld) {
        $runtimeMutex.ReleaseMutex()
    }
    $runtimeMutex.Dispose()
}
