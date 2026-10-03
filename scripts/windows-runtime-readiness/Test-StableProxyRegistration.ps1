[CmdletBinding()]
param([string]$ProductionScript = (Join-Path $PSScriptRoot 'Repair-CodexCuaRuntime.ps1'))
$ErrorActionPreference = 'Stop'
$taskRoot = Join-Path $PSScriptRoot ('fixtures\' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskRoot -Force | Out-Null
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($ProductionScript, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Production script has syntax errors.' }
$definition = $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Register-StableCuaProxyRuntime' }, $true)
if ($definition.Count -ne 1) { throw 'Production registration function is missing.' }
. ([scriptblock]::Create($definition[0].Extent.Text))
$script:valid = $true
$script:mockProcesses = @()
function Test-CuaRuntime { param($CandidateRoot,$Fingerprint,$ProxySpec) return $script:valid }
function Get-CurrentCodexPackage { [pscustomobject]@{InstallLocation='C:\fixture-package'} }
function Get-CimInstance { param($ClassName,$Filter) return $script:mockProcesses }
function Write-RepairLog { param($Message) }
function Assert-True { param($Condition,$Message) if (-not $Condition) { throw $Message } }
function Assert-Throws { param([scriptblock]$Action) try { & $Action } catch { return }; throw 'Expected rejection.' }
$results = [Collections.Generic.List[string]]::new()
$runtimeRoot = Join-Path $taskRoot 'managed'
$stable = Join-Path $taskRoot 'stable'
New-Item -ItemType Directory -Path $runtimeRoot,$stable | Out-Null
$fixtureId = '0123456789abcdef'
$link = Join-Path $runtimeRoot $fixtureId
Register-StableCuaProxyRuntime $runtimeRoot $stable $fixtureId @{} @{}
Assert-True ((Get-Item -LiteralPath $link).LinkType -eq 'Junction') 'New runtime was not registered as a junction.'
$results.Add('new runtime registers after validation')
Register-StableCuaProxyRuntime $runtimeRoot $stable $fixtureId @{} @{}
Assert-True ((Get-ChildItem -LiteralPath $runtimeRoot).Count -eq 1) 'Reuse modified the directory.'
$results.Add('correct junction is reused')
$otherStable = Join-Path $taskRoot 'other-stable'
New-Item -ItemType Directory -Path $otherStable | Out-Null
Assert-Throws { Register-StableCuaProxyRuntime $runtimeRoot $otherStable $fixtureId @{} @{} }
Assert-True ([string](Get-Item -LiteralPath $link).Target -eq $stable) 'Unexpected junction target was changed.'
$results.Add('wrong junction is rejected without replacement')
Assert-Throws { Register-StableCuaProxyRuntime $runtimeRoot $stable '../outside' @{} @{} }
$results.Add('escaping runtime identifier is rejected')
$script:valid = $false
Assert-Throws { Register-StableCuaProxyRuntime $runtimeRoot $stable '2222222222222222' @{} @{} }
Assert-True (-not (Test-Path -LiteralPath (Join-Path $runtimeRoot '2222222222222222'))) 'Unverified runtime was registered.'
$results.Add('failed validation creates no link')
$script:valid = $true
$legacyId = '3333333333333333'
$legacy = Join-Path $runtimeRoot $legacyId
New-Item -ItemType Directory -Path $legacy | Out-Null
[pscustomobject]@{runtimeId=$legacyId;runtimeKind='cua-node-repl-proxy'} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $legacy '.codex-runtime-sync.json')
Set-Content -LiteralPath (Join-Path $legacy 'preserved.txt') -Value 'original'
$script:mockProcesses = @([pscustomobject]@{ExecutablePath=(Join-Path $legacy 'bin\node.exe')})
Assert-Throws { Register-StableCuaProxyRuntime $runtimeRoot $stable $legacyId @{} @{} }
Assert-True (Test-Path -LiteralPath (Join-Path $legacy 'preserved.txt')) 'An in-use runtime was moved.'
$results.Add('in-use legacy runtime is preserved')
$script:mockProcesses = @()
Register-StableCuaProxyRuntime $runtimeRoot $stable $legacyId @{} @{}
$backup = @(Get-ChildItem -LiteralPath $runtimeRoot -Directory -Filter ($legacyId + '.backup-stable-proxy-*'))
Assert-True ($backup.Count -eq 1 -and (Test-Path -LiteralPath (Join-Path $backup[0].FullName 'preserved.txt'))) 'Legacy backup is missing.'
Assert-True ((Get-Item -LiteralPath $legacy).LinkType -eq 'Junction') 'Legacy runtime was not migrated.'
$results.Add('unused legacy runtime migrates with recoverable backup')
$unknownId = '4444444444444444'
$unknown = Join-Path $runtimeRoot $unknownId
New-Item -ItemType Directory -Path $unknown | Out-Null
[pscustomobject]@{runtimeId=$unknownId;runtimeKind='unknown'} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $unknown '.codex-runtime-sync.json')
Assert-Throws { Register-StableCuaProxyRuntime $runtimeRoot $stable $unknownId @{} @{} }
Assert-True ((Get-Item -LiteralPath $unknown).LinkType -ne 'Junction') 'An unknown directory was replaced.'
$results.Add('unrecognized existing directory remains intact')
[pscustomobject]@{Passed=$results.Count;Cases=$results;FixtureRoot=$taskRoot} | ConvertTo-Json -Depth 3
