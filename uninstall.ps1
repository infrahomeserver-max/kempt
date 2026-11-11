<#
.SYNOPSIS
  Kempt uninstaller for Windows: removes exactly what install.ps1 recorded, then any folders it created (if empty).

.EXAMPLE
  .\uninstall.ps1            # asks first
  .\uninstall.ps1 -DryRun    # show what would be removed
  .\uninstall.ps1 -Yes       # no questions
#>
[CmdletBinding()]
param(
  [string]$Prefix,       # the install folder (default: the folder this script is in)
  [switch]$Yes,
  [switch]$DryRun,
  [switch]$PurgeSettings,   # also delete Kempt's saved settings (%APPDATA%\kempt)
  [switch]$Relaunched    # internal
)
$ErrorActionPreference = 'Stop'
function Say($m)  { Write-Host "==> $m" -ForegroundColor Cyan }
function Warn($m) { Write-Host "warning: $m" -ForegroundColor Yellow }
$IsWin = ($env:OS -eq 'Windows_NT') -and -not $env:KEMPT_TEST_NOWIN

if (-not $Prefix) { $Prefix = $PSScriptRoot }
$Prefix = $Prefix.TrimEnd('\','/')
$Manifest = Join-Path $Prefix 'manifest.txt'

# a script cannot delete the folder it runs from: continue from a temporary copy
if (-not $Relaunched -and -not $DryRun) {
  $tmp = Join-Path ([IO.Path]::GetTempPath()) ("kempt-uninstall-" + [guid]::NewGuid().ToString('N') + '.ps1')
  Copy-Item -LiteralPath $PSCommandPath -Destination $tmp
  $argl = @('-NoProfile','-ExecutionPolicy','Bypass','-File',$tmp,'-Prefix',$Prefix,'-Relaunched')
  if ($Yes) { $argl += '-Yes' }
  if ($PurgeSettings) { $argl += '-PurgeSettings' }
  $exe = (Get-Process -Id $PID).Path
  & $exe @argl
  $rc = $LASTEXITCODE
  Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
  exit $rc
}

if (-not (Test-Path -LiteralPath $Manifest)) { Write-Host "Kempt does not seem to be installed in $Prefix (no manifest.txt)."; exit 1 }
$lines = Get-Content -LiteralPath $Manifest
$files = @($lines | Where-Object { $_ -like 'file:*' } | ForEach-Object { $_.Substring(5) })
$dirs  = @($lines | Where-Object { $_ -like 'dir:*'  } | ForEach-Object { $_.Substring(4) } | Sort-Object Length -Descending)
$paths = @($lines | Where-Object { $_ -like 'path:*' } | ForEach-Object { $_.Substring(5) })
$regs  = @($lines | Where-Object { $_ -like 'reg:*'  } | ForEach-Object { $_.Substring(4) })

Say "Kempt in $Prefix will be removed:"
foreach ($f in $files) { if (Test-Path -LiteralPath $f) { Write-Host "  $f" } }
if (Test-Path (Join-Path $Prefix 'tools')) { Write-Host "  $(Join-Path $Prefix 'tools')\   (helper formatters)" }
foreach ($p in $paths) { Write-Host "  PATH entry: $($p.Split('|')[1])" }
foreach ($r in $regs)  { Write-Host "  registry:   $r" }
if ($DryRun) { Write-Host "(dry run: nothing was removed)"; exit 0 }
if (-not $Yes) { $a = Read-Host 'Continue? [y/N]'; if ($a -notmatch '^[Yy]') { Write-Host 'aborted'; exit 1 } }

# stop a running Kempt from this folder so its files can be deleted
if ($IsWin) {
  Get-Process kempt -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($Prefix, [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { try { $_.Kill(); $_.WaitForExit(3000) | Out-Null } catch { } }
  foreach ($r in $regs) { if (Test-Path -LiteralPath $r) { Remove-Item -LiteralPath $r -Recurse -Force } }
  foreach ($p in $paths) {
    $scope, $dir = $p.Split('|', 2)
    $cur = [Environment]::GetEnvironmentVariable('Path', $scope)
    if ($cur) { [Environment]::SetEnvironmentVariable('Path', (($cur.Split(';') | Where-Object { $_ -and ($_ -ne $dir) }) -join ';'), $scope) }
  }
}
Set-Location ([IO.Path]::GetTempPath())
foreach ($f in $files) {
  if ($f -eq $Manifest) { continue }
  try { Remove-Item -LiteralPath $f -Force -ErrorAction Stop } catch { if (Test-Path -LiteralPath $f) { Warn "could not delete $f (is Kempt still running? close its console window and try again)" } }
}
$tools = Join-Path $Prefix 'tools'
if (Test-Path -LiteralPath $tools) { Remove-Item -LiteralPath $tools -Recurse -Force -ErrorAction SilentlyContinue }
Remove-Item -LiteralPath $Manifest -Force -ErrorAction SilentlyContinue
foreach ($d in $dirs) { if ((Test-Path -LiteralPath $d) -and -not (Get-ChildItem -LiteralPath $d -Force -ErrorAction SilentlyContinue)) { Remove-Item -LiteralPath $d -Force -ErrorAction SilentlyContinue } }
Say "Kempt has been removed."
$cfg = if ($env:KEMPT_CONFIG_DIR) { $env:KEMPT_CONFIG_DIR } elseif ($env:APPDATA) { Join-Path $env:APPDATA 'kempt' } else { Join-Path $HOME '.config/kempt' }
if (Test-Path -LiteralPath (Join-Path $cfg 'state.json')) {
  if ($PurgeSettings) { Remove-Item -LiteralPath $cfg -Recurse -Force -ErrorAction SilentlyContinue; Write-Host "Saved settings deleted ($cfg)." }
  else { Write-Host "Your saved settings (theme, last folder, recent files) were kept in $cfg - run again with -PurgeSettings to delete them." }
}
