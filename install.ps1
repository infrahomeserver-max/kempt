<#
.SYNOPSIS
  Kempt installer for Windows 10 / 11 (x64, also runs on Windows 11 ARM64 through emulation).

.DESCRIPTION
  Installs Kempt for the current user (no administrator rights) or for everyone (-System).
  Every file, folder, shortcut, PATH entry and registry key it creates is recorded in a manifest, so
  uninstall.ps1 (or "Apps & features") removes exactly that and leaves the machine as it was.

.EXAMPLE
  .\install.ps1                 # per-user install into %LOCALAPPDATA%\Programs\Kempt
  .\install.ps1 -DryRun         # show what would happen, change nothing
  .\install.ps1 -System         # all users into %ProgramFiles%\Kempt (run PowerShell as administrator)
  .\install.ps1 -WithTools      # also install prettier (Node.js) and ruff + sqlformat (Python) privately
  .\install.ps1 -ContextMenu    # add "Open with Kempt" to the right-click menu of files
#>
[CmdletBinding()]
param(
  [switch]$System,        # install for all users (needs an elevated PowerShell)
  [string]$Prefix,        # install into this folder instead
  [switch]$WithTools,     # formatters for other languages (private to Kempt)
  [switch]$NoShortcut,    # skip the Start-menu entry
  [switch]$NoPath,        # do not add the install folder to PATH
  [switch]$ContextMenu,   # add "Open with Kempt" to the Explorer right-click menu
  [switch]$DryRun         # change nothing, just print the plan
)
$ErrorActionPreference = 'Stop'
$Version = '0.3.2'

function Say($m)  { Write-Host "==> $m" -ForegroundColor Cyan }
function Warn($m) { Write-Host "warning: $m" -ForegroundColor Yellow }
function Plan($m) { Write-Host "    $m" -ForegroundColor DarkGray }
function Die($m)  { Write-Host "error: $m" -ForegroundColor Red; exit 1 }

# KEMPT_TEST_NOWIN lets the file logic be tested on a non-Windows machine (no registry / shortcuts / PATH there)
$IsWin = ($env:OS -eq 'Windows_NT') -and -not $env:KEMPT_TEST_NOWIN

# ---------------------------------------------------------------- detect the environment
$build = 0; $caption = 'Windows'; $arch = 'AMD64'
if ($IsWin) {
  $build = [Environment]::OSVersion.Version.Build
  try { $caption = (Get-CimInstance Win32_OperatingSystem).Caption } catch { $caption = 'Windows' }
  $arch = $env:PROCESSOR_ARCHITECTURE
  if ($env:PROCESSOR_ARCHITEW6432) { $arch = $env:PROCESSOR_ARCHITEW6432 }
  Say "Detected: $caption (build $build) - $arch"
  if ($build -lt 10240) { Die "Windows 10 or newer is required (this is build $build)." }
  if ($arch -eq 'ARM64' -and $build -lt 22000) { Die "This is the 64-bit Intel/AMD build. On ARM it needs Windows 11 (x64 emulation); this is Windows 10 on ARM." }
  if ($arch -eq 'x86') { Die "32-bit Windows is not supported." }
  if ($arch -eq 'ARM64') { Say "ARM64 detected: the x64 build runs through Windows' built-in emulation." }
} else { Say "Test mode (no Windows APIs)" }

$isAdmin = $false
if ($IsWin) { $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) }
if ($System -and $IsWin -and -not $isAdmin) { Die "-System needs an elevated PowerShell. Right-click PowerShell, choose 'Run as administrator', and run this again (or omit -System for a per-user install)." }

# ---------------------------------------------------------------- where to install
if (-not $Prefix) {
  if ($System) { $Prefix = Join-Path $env:ProgramFiles 'Kempt' } else { $Prefix = Join-Path $env:LOCALAPPDATA 'Programs\Kempt' }
}
$Prefix = $Prefix.TrimEnd('\','/')
$Scope = 'user'; if ($System) { $Scope = 'machine' }
$Manifest = Join-Path $Prefix 'manifest.txt'
$Here = $PSScriptRoot
$Exe = Join-Path $Here 'kempt.exe'
if (-not (Test-Path $Exe)) { Die "kempt.exe was not found next to install.ps1 ($Here)." }
if ($IsWin -and -not $DryRun) {
  try { $v = (& $Exe --version) 2>&1 } catch { $v = '' }
  if ("$v" -notmatch '^kempt ') { Die "kempt.exe does not run on this PC ($v). Download the build that matches your system." }
}
if ((Test-Path $Manifest) -and -not $DryRun) { Say "Kempt is already installed in $Prefix; upgrading in place" }

# ---------------------------------------------------------------- helpers that record what we create
$Log = New-Object System.Collections.ArrayList
$Planned = @{}
function Rec($line) { [void]$Log.Add($line) }
function EnsureDir($d) {
  $stack = @(); $cur = $d
  while ($cur -and -not (Test-Path -LiteralPath $cur) -and -not ($DryRun -and $Planned.ContainsKey("rec:$cur"))) { $stack += $cur; $cur = Split-Path -Parent $cur }
  [array]::Reverse($stack)
  foreach ($s in $stack) {
    if ($DryRun) { if (-not $Planned.ContainsKey($s)) { $Planned[$s] = 1; Plan "mkdir $s" } } else { New-Item -ItemType Directory -Path $s | Out-Null }
    if (-not $DryRun -or -not $Planned.ContainsKey("rec:$s")) { $Planned["rec:$s"] = 1; Rec "dir:$s" }
  }
}
function Put($src, $destName) {
  $dest = Join-Path $Prefix $destName
  EnsureDir (Split-Path -Parent $dest)
  if ($DryRun) { Plan "copy  $dest" } else {
    try { Copy-Item -LiteralPath $src -Destination $dest -Force }
    catch { Die "could not write $dest. If Kempt is running, close it (and its black console window) and run this again." }
  }
  Rec "file:$dest"
}

# ---------------------------------------------------------------- 1. files
Say "Installing into $Prefix"
Put $Exe 'kempt.exe'
foreach ($f in 'uninstall.ps1','uninstall.cmd','README.md') { $p = Join-Path $Here $f; if (Test-Path $p) { Put $p $f } }
$IcoSrc = Join-Path $Here 'packaging\kempt.ico'; if (-not (Test-Path $IcoSrc)) { $IcoSrc = Join-Path $Here 'kempt.ico' }
$Ico = Join-Path $Prefix 'kempt.ico'
if (Test-Path $IcoSrc) { Put $IcoSrc 'kempt.ico' }

# ---------------------------------------------------------------- 2. PATH, Start menu, Apps & features
if ($IsWin) {
  if (-not $NoPath) {
    $cur = [Environment]::GetEnvironmentVariable('Path', $Scope)
    $parts = @(); if ($cur) { $parts = $cur.Split(';') | Where-Object { $_ } }
    if ($parts -notcontains $Prefix) {
      if ($DryRun) { Plan "add $Prefix to the $Scope PATH" } else { [Environment]::SetEnvironmentVariable('Path', (($parts + $Prefix) -join ';'), $Scope) }
      Rec "path:$Scope|$Prefix"
    }
  }
  if (-not $NoShortcut) {
    if ($System) { $menu = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Kempt' } else { $menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Kempt' }
    EnsureDir $menu
    $lnk = Join-Path $menu 'Kempt.lnk'
    if ($DryRun) { Plan "shortcut $lnk" } else {
      $sh = New-Object -ComObject WScript.Shell
      $s = $sh.CreateShortcut($lnk)
      $s.TargetPath = Join-Path $Prefix 'kempt.exe'; $s.Arguments = 'gui'; $s.WorkingDirectory = $env:USERPROFILE
      $s.WindowStyle = 7; $s.Description = 'View, search, fix and compare big JSON, XML and log files'   # 7 = minimised console
      if (Test-Path $Ico) { $s.IconLocation = $Ico }
      $s.Save()
    }
    Rec "file:$lnk"
  }
  $hive = 'HKCU:'; if ($System) { $hive = 'HKLM:' }
  $uk = "$hive\Software\Microsoft\Windows\CurrentVersion\Uninstall\Kempt"
  if ($DryRun) { Plan "Apps & features entry $uk" } else {
    New-Item -Path $uk -Force | Out-Null
    $props = @{
      DisplayName = 'Kempt'; DisplayVersion = $Version; Publisher = 'Kempt'; InstallLocation = $Prefix
      UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $Prefix 'uninstall.ps1')`""
    }
    if (Test-Path $Ico) { $props['DisplayIcon'] = $Ico }
    foreach ($k in $props.Keys) { Set-ItemProperty -Path $uk -Name $k -Value $props[$k] }
    Set-ItemProperty -Path $uk -Name NoModify -Value 1 -Type DWord; Set-ItemProperty -Path $uk -Name NoRepair -Value 1 -Type DWord
  }
  Rec "reg:$uk"
  if ($ContextMenu) {
    $ck = "$hive\Software\Classes\*\shell\Kempt"
    if ($DryRun) { Plan "context menu $ck" } else {
      New-Item -Path "$ck\command" -Force | Out-Null
      Set-ItemProperty -Path $ck -Name '(default)' -Value 'Open with Kempt'
      if (Test-Path $Ico) { Set-ItemProperty -Path $ck -Name Icon -Value $Ico }
      Set-ItemProperty -Path "$ck\command" -Name '(default)' -Value "`"$(Join-Path $Prefix 'kempt.exe')`" gui `"%1`""
    }
    Rec "reg:$ck"
  }
}

# ---------------------------------------------------------------- 3. optional formatter tools (private to Kempt)
function VerGe($a, $b) { try { return ([version]$a -ge [version]$b) } catch { return $false } }
if ($WithTools) {
  $Tools = Join-Path $Prefix 'tools'
  Say "Formatter tools (private to Kempt, in $Tools)"
  if (-not $DryRun) { EnsureDir $Tools }
  $node = Get-Command node -ErrorAction SilentlyContinue; $npm = Get-Command npm -ErrorAction SilentlyContinue
  if ($node -and $npm) {
    $nv = (& node --version).TrimStart('v'); $major = [int]($nv.Split('.')[0]); $pkg = ''
    if ($major -ge 18) { $pkg = 'prettier' } elseif ($major -ge 14) { $pkg = 'prettier@3.0.3' } elseif ($major -ge 10) { $pkg = 'prettier@2.8.8' }
    if (-not $pkg) { Warn "Node.js $nv is too old for prettier (need 10+): skipping" }
    else {
      if ($pkg -ne 'prettier') { Say "Node.js $nv is older than 18: using $pkg" }
      if ($DryRun) { Plan "npm install $pkg -> $Tools" } else {
        $nc = Join-Path ([IO.Path]::GetTempPath()) ("kempt-npm-" + [guid]::NewGuid().ToString('N'))
        try { & npm install --prefix $Tools --cache $nc --no-audit --no-fund --loglevel=error $pkg } catch { Warn "prettier install failed: $_" }
        Remove-Item -Recurse -Force $nc -ErrorAction SilentlyContinue
      }
    }
  } else { Warn "Node.js not found: skipping prettier (HTML/CSS/JS/YAML/Markdown). Install it from https://nodejs.org or: winget install OpenJS.NodeJS.LTS" }
  $py = Get-Command python -ErrorAction SilentlyContinue; if (-not $py) { $py = Get-Command py -ErrorAction SilentlyContinue }
  if ($py) {
    if ($DryRun) { Plan "python venv + pip install ruff sqlparse -> $Tools\venv" } else {
      try {
        & $py.Source -m venv (Join-Path $Tools 'venv')
        & (Join-Path $Tools 'venv\Scripts\python.exe') -m pip install --quiet --no-cache-dir --disable-pip-version-check ruff sqlparse
      } catch { Warn "ruff/sqlformat install failed: $_" }
    }
  } else { Warn "Python not found: skipping ruff and sqlformat. Install from https://python.org or: winget install Python.Python.3.12" }
  Warn "For shell and C/C++/Java formatting install: winget install mvdan.shfmt LLVM.LLVM   (not done automatically: they live outside Kempt's folder)"
}

# ---------------------------------------------------------------- 4. manifest
if (-not $DryRun) {
  $kept = @(); if (Test-Path $Manifest) { $kept = Get-Content $Manifest | Where-Object { $_ -match '^(dir|file|path|reg):' } }
  $lines = @('# kempt install manifest - used by uninstall.ps1. Do not edit.', "version:$Version", "prefix:$Prefix", "scope:$Scope", "system:$caption $arch") + $kept + @($Log) + @("file:$Manifest")
  $seen = @{}; $out = @()
  foreach ($l in $lines) { if (-not $seen.ContainsKey($l)) { $seen[$l] = 1; $out += $l } }
  Set-Content -LiteralPath $Manifest -Value $out -Encoding UTF8
}

Say "Done."
if ($DryRun) { Write-Host "(dry run: nothing was changed)"; exit 0 }
if ($IsWin) {
  Write-Host ""
  Write-Host "Start it from the Start menu (Kempt), or in a NEW terminal window:  kempt gui app.log"
  Write-Host "Remove it with Settings > Apps > Kempt > Uninstall, or:  $(Join-Path $Prefix 'uninstall.cmd')"
  & (Join-Path $Prefix 'kempt.exe') doctor
}
