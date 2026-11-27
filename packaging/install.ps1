<#
.SYNOPSIS
  One-line installer for Kempt on Windows 10 / 11. Downloads the newest release, checks it against SHA256SUMS
  and runs the real installer from the zip.

.EXAMPLE
  irm https://github.com/infrahomeserver-max/kempt/releases/latest/download/install.ps1 | iex
  & ([scriptblock]::Create((irm https://github.com/infrahomeserver-max/kempt/releases/latest/download/install.ps1))) -NoContextMenu
#>
[CmdletBinding()]
param(
  [string]$Tag,               # e.g. v0.2.4 (default: newest)
  [switch]$NoContextMenu,     # skip the "Open with Kempt" right-click entry (added by default)
  [switch]$System,            # all users (needs an administrator PowerShell)
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is far faster without the progress bar
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$Repo = 'infrahomeserver-max/kempt'

function Say($m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Die($m) { Write-Host "error: $m" -ForegroundColor Red; throw $m }

if ($env:OS -ne 'Windows_NT') { Die 'This installer is for Windows. On Linux/macOS use install.sh.' }

$base = if ($Tag) { "https://github.com/$Repo/releases/download/$Tag" } else { "https://github.com/$Repo/releases/latest/download" }
$work = Join-Path $env:TEMP ('kempt-install-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory $work | Out-Null
try {
  Say 'Looking up the release'
  $sums = (Invoke-WebRequest "$base/SHA256SUMS" -UseBasicParsing).Content
  if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
  $line = ($sums -split "`n") | Where-Object { $_ -match 'kempt-.*-windows-x86_64\.zip' } | Select-Object -First 1
  if (-not $line) { Die 'No Windows build was found in this release.' }
  $hash = ($line.Trim() -split '\s+')[0].ToLower()
  $name = ($line.Trim() -split '\s+')[-1].TrimStart('*')

  Say "Downloading $name"
  $zip = Join-Path $work $name
  Invoke-WebRequest "$base/$name" -OutFile $zip -UseBasicParsing

  Say 'Checking the download'
  $got = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
  if ($got -ne $hash) { Die "Checksum mismatch (expected $hash, got $got). Nothing was installed." }

  Expand-Archive $zip -DestinationPath $work -Force
  $inst = Get-ChildItem $work -Recurse -Filter install.ps1 | Select-Object -First 1
  if (-not $inst) { Die 'install.ps1 was not found inside the zip.' }

  $args2 = @{}
  if (-not $NoContextMenu) { $args2.ContextMenu = $true }
  if ($System) { $args2.System = $true }
  if ($DryRun) { $args2.DryRun = $true }
  Say 'Running the installer'
  & powershell -NoProfile -ExecutionPolicy Bypass -File $inst.FullName @(foreach ($k in $args2.Keys) { "-$k" })
  if ($LASTEXITCODE) { Die "The installer failed (exit $LASTEXITCODE)." }
} finally {
  Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
