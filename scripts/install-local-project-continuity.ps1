[CmdletBinding()]
param([switch]$DryRun)
$ErrorActionPreference='Stop'
$source=Join-Path (Split-Path -Parent $PSScriptRoot) 'tools\local-project-continuity'
$configRoot=Join-Path $env:APPDATA 'Codex++'
$destination=Join-Path $configRoot 'user_scripts\local-project-continuity'
$configPath=Join-Path $configRoot 'user_scripts.json'
$stamp=(Get-Date).ToString('yyyyMMdd-HHmmss')
$backupDirectory=Join-Path $configRoot ('extension-data\local-project-continuity\installation-backups\'+$stamp)
$files=@('codexpp.json','core.js','renderer.js','backend.mjs')
foreach($name in $files){if(-not (Test-Path -LiteralPath (Join-Path $source $name))){throw ('Missing source: '+$name)}}
$config=Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json -AsHashtable
if($config.enabled -ne $true){throw 'User extensions are disabled; installation will not enable unrelated extensions.'}
if($DryRun){[pscustomobject]@{Source=$source;Destination=$destination;Config=$configPath;Backup=$backupDirectory;Files=$files} | ConvertTo-Json;return}
New-Item -ItemType Directory -Path $backupDirectory -Force | Out-Null
Copy-Item -LiteralPath $configPath -Destination (Join-Path $backupDirectory 'user_scripts.json')
New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach($name in $files){
    $target=Join-Path $destination $name
    if(Test-Path -LiteralPath $target){Copy-Item -LiteralPath $target -Destination (Join-Path $backupDirectory $name)}
    Copy-Item -LiteralPath (Join-Path $source $name) -Destination $target -Force
}
$config.scripts['user:local-project-continuity']=$true
$temporary=$configPath+'.continuity-'+[guid]::NewGuid().ToString('N')+'.tmp'
$config | ConvertTo-Json -Depth 50 | Set-Content -LiteralPath $temporary -Encoding utf8NoBOM
Move-Item -LiteralPath $temporary -Destination $configPath -Force
[pscustomobject]@{Installed=$destination;Backup=$backupDirectory} | ConvertTo-Json
