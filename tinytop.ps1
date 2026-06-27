Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$TinyTopVersion = "0.1.34"
$ServiceName = "TinyTop"
$DefaultHost = if ($env:HOST) { $env:HOST } else { "127.0.0.1" }
$DefaultPort = if ($env:PORT) { [int]$env:PORT } else { 4274 }
$RootDir = Split-Path -Parent $MyInvocation.MyCommand.Path

function Get-TinyTopBaseDir {
  if ($env:LOCALAPPDATA) {
    return Join-Path $env:LOCALAPPDATA "TinyTop"
  }

  return Join-Path $HOME "AppData\Local\TinyTop"
}

function Get-TinyTopBinDir {
  return Join-Path (Get-TinyTopBaseDir) "bin"
}

function Get-TinyTopStateDir {
  return Join-Path (Get-TinyTopBaseDir) "state"
}

function Get-TinyTopLogDir {
  return Join-Path (Get-TinyTopBaseDir) "logs"
}

function Get-TinyTopAgentPath {
  return Join-Path (Get-TinyTopBinDir) "tinytop-agent.exe"
}

function Get-TinyTopPidPath {
  return Join-Path (Get-TinyTopStateDir) "tinytop.pid"
}

function Get-TinyTopLogPath {
  return Join-Path (Get-TinyTopLogDir) "tinytop.log"
}

function Get-TinyTopErrorLogPath {
  return Join-Path (Get-TinyTopLogDir) "tinytop.err.log"
}

function Get-TinyTopSqlitePath {
  if ($env:TINYTOP_HISTORY_DB) {
    return $env:TINYTOP_HISTORY_DB
  }

  return Join-Path (Get-TinyTopStateDir) "history.sqlite"
}

function New-TinyTopDirectories {
  New-Item -ItemType Directory -Force -Path (Get-TinyTopBinDir), (Get-TinyTopStateDir), (Get-TinyTopLogDir) | Out-Null
}

function Get-TinyTopReleaseArch {
  if ([Environment]::Is64BitOperatingSystem) {
    return "x86_64"
  }

  return "x86"
}

function Get-TinyTopReleaseAssetName {
  return "tinytop-agent-windows-$(Get-TinyTopReleaseArch).exe"
}

function Get-TinyTopReleaseUrl {
  return "https://github.com/michelabboud/tinytop/releases/download/v$TinyTopVersion/$(Get-TinyTopReleaseAssetName)"
}

function Write-TinyTopHelp {
  @"
TinyTop $TinyTopVersion
Native Windows command center for the Rust collector/dashboard daemon.

Usage:
  .\tinytop.ps1 help
  .\tinytop.ps1 doctor
  .\tinytop.ps1 rust install-binary
  .\tinytop.ps1 rust build
  .\tinytop.ps1 start
  .\tinytop.ps1 stop
  .\tinytop.ps1 restart
  .\tinytop.ps1 status
  .\tinytop.ps1 logs
  .\tinytop.ps1 service install
  .\tinytop.ps1 service uninstall
  .\tinytop.ps1 service start|stop|restart|status

Dashboard:
  http://$DefaultHost`:$DefaultPort

Windows paths:
  Binary:  $(Get-TinyTopAgentPath)
  State:   $(Get-TinyTopStateDir)
  Logs:    $(Get-TinyTopLogPath)
  Errors:  $(Get-TinyTopErrorLogPath)
  SQLite:  $(Get-TinyTopSqlitePath)
"@
}

function Test-TinyTopCommand {
  param([Parameter(Mandatory = $true)][string]$Name)
  return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Test-TinyTopAdmin {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = [Security.Principal.WindowsPrincipal]::new($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Confirm-TinyTopServiceElevation {
  param([Parameter(Mandatory = $true)][string]$Action)

  if (Test-TinyTopAdmin) {
    return
  }

  $message = "TinyTop service '$Action' usually requires an elevated PowerShell session."
  if (-not [Environment]::UserInteractive) {
    throw "$message Run PowerShell as Administrator, or rerun interactively to confirm before continuing."
  }

  Write-Warning "$message Run PowerShell as Administrator for the safest path."
  [string]$answer = Read-Host "PowerShell is not elevated. Continue with service $Action anyway? [y/N]"
  if ($answer.Trim() -notmatch "(?i)^(y|yes)$") {
    throw "Cancelled TinyTop service $Action. Run PowerShell as Administrator and retry."
  }
}

function Get-TinyTopRunnableAgent {
  $localAgent = Get-TinyTopAgentPath
  if (Test-Path $localAgent) {
    return $localAgent
  }

  $targetAgent = Join-Path $RootDir "agent\target\release\tinytop-agent.exe"
  if (Test-Path $targetAgent) {
    return $targetAgent
  }

  return $localAgent
}

function Install-TinyTopRustBinary {
  New-TinyTopDirectories
  $url = Get-TinyTopReleaseUrl
  $destination = Get-TinyTopAgentPath
  Write-Host "Downloading TinyTop Rust collector binary:"
  Write-Host "  $url"
  try {
    Invoke-WebRequest -Uri $url -OutFile $destination
  } catch {
    throw "Could not download a prebuilt TinyTop Rust collector binary for this platform. Install Rust and run .\tinytop.ps1 rust build. $($_.Exception.Message)"
  }
  Write-Host "Installed TinyTop Rust collector binary: $destination"
}

function Invoke-TinyTopRustBuild {
  param([switch]$PrintCommand)

  $cargoArgs = @(
    "build",
    "--release",
    "--manifest-path",
    "agent/Cargo.toml",
    "-p",
    "tinytop-agent",
    "--no-default-features",
    "--features",
    "windows-collector"
  )

  if ($PrintCommand) {
    Write-Host ("cargo " + ($cargoArgs -join " "))
    return
  }

  if (-not (Test-TinyTopCommand "cargo")) {
    throw "Rust/Cargo is required. Install Rust from https://rustup.rs, then rerun .\tinytop.ps1 rust build."
  }

  Push-Location $RootDir
  try {
    & cargo @cargoArgs
  } finally {
    Pop-Location
  }
}

function Start-TinyTop {
  New-TinyTopDirectories
  $agent = Get-TinyTopRunnableAgent
  if (-not (Test-Path $agent)) {
    throw "Rust collector binary is missing. Run .\tinytop.ps1 rust install-binary or .\tinytop.ps1 rust build."
  }

  $logPath = Get-TinyTopLogPath
  $errorLogPath = Get-TinyTopErrorLogPath
  $sqlitePath = Get-TinyTopSqlitePath
  $args = @("serve", "--host", $DefaultHost, "--port", "$DefaultPort", "--sqlite", $sqlitePath)
  $process = Start-Process -FilePath $agent -ArgumentList $args -WorkingDirectory $RootDir -RedirectStandardOutput $logPath -RedirectStandardError $errorLogPath -PassThru -WindowStyle Hidden
  Set-Content -Path (Get-TinyTopPidPath) -Value $process.Id -Encoding ascii
  Write-Host "Started TinyTop on http://$DefaultHost`:$DefaultPort"
  Write-Host "PID: $($process.Id)"
  Write-Host "Log: $logPath"
  Write-Host "Error log: $errorLogPath"
}

function Stop-TinyTop {
  $pidPath = Get-TinyTopPidPath
  if (-not (Test-Path $pidPath)) {
    Write-Host "No TinyTop PID file found."
    return
  }

  $processId = [int](Get-Content $pidPath -Raw)
  $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
  if ($process) {
    Stop-Process -Id $processId
    Write-Host "Stopped TinyTop process $processId."
  } else {
    Write-Host "TinyTop process $processId is not running."
  }
  Remove-Item $pidPath -Force -ErrorAction SilentlyContinue
}

function Restart-TinyTop {
  Stop-TinyTop
  Start-TinyTop
}

function Get-TinyTopStatus {
  $url = "http://$DefaultHost`:$DefaultPort/api/version"
  try {
    $version = Invoke-RestMethod -Uri $url -TimeoutSec 2
    Write-Host "Running daemon: $($version.runtime) $($version.component) v$($version.version) ($($version.dashboard) dashboard)"
  } catch {
    Write-Host "Running daemon: unknown ($url unavailable)"
  }

  $agent = Get-TinyTopRunnableAgent
  if (Test-Path $agent) {
    Write-Host "Rust collector binary: ok ($agent)"
  } else {
    Write-Host "Rust collector binary: missing (run .\tinytop.ps1 rust install-binary or .\tinytop.ps1 rust build)"
  }

  $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
  if ($service) {
    Write-Host "Windows service: $($service.Status)"
  } else {
    Write-Host "Windows service: not installed"
  }
}

function Show-TinyTopLogs {
  $logPath = Get-TinyTopLogPath
  $errorLogPath = Get-TinyTopErrorLogPath
  $paths = @($logPath, $errorLogPath) | Where-Object { Test-Path $_ }
  if ($paths.Count -eq 0) {
    Write-Host "No TinyTop logs found at $logPath or $errorLogPath"
    return
  }

  Get-Content -Path $paths -Tail 80 -Wait
}

function Install-TinyTopService {
  Confirm-TinyTopServiceElevation -Action "install"
  New-TinyTopDirectories
  $agent = Get-TinyTopRunnableAgent
  if (-not (Test-Path $agent)) {
    throw "Rust collector binary is missing. Run .\tinytop.ps1 rust install-binary or .\tinytop.ps1 rust build first."
  }

  $sqlitePath = Get-TinyTopSqlitePath
  $binaryPath = "`"$agent`" serve --host $DefaultHost --port $DefaultPort --sqlite `"$sqlitePath`""
  $existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
  if ($existing) {
    throw "TinyTop service already exists. Run .\tinytop.ps1 service uninstall first."
  }

  New-Service -Name $ServiceName -BinaryPathName $binaryPath -DisplayName "TinyTop" -Description "TinyTop Rust collector and dashboard daemon" -StartupType Automatic
  Write-Host "Installed TinyTop Windows service."
  Write-Host "Start with: .\tinytop.ps1 service start"
}

function Uninstall-TinyTopService {
  Confirm-TinyTopServiceElevation -Action "uninstall"
  $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
  if ($service) {
    if ($service.Status -ne "Stopped") {
      Stop-Service -Name $ServiceName
    }
    & sc.exe delete $ServiceName | Out-Null
    Write-Host "Removed TinyTop Windows service."
  } else {
    Write-Host "TinyTop Windows service is not installed."
  }
}

function Invoke-TinyTopService {
  param([Parameter(Mandatory = $true)][string]$Action)

  switch ($Action) {
    "install" { Install-TinyTopService }
    "uninstall" { Uninstall-TinyTopService }
    "start" {
      Confirm-TinyTopServiceElevation -Action "start"
      Start-Service -Name $ServiceName
    }
    "stop" {
      Confirm-TinyTopServiceElevation -Action "stop"
      Stop-Service -Name $ServiceName
    }
    "restart" {
      Confirm-TinyTopServiceElevation -Action "restart"
      Stop-Service -Name $ServiceName
      Start-Service -Name $ServiceName
    }
    "status" {
      $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
      if ($service) {
        $service | Format-List Name, DisplayName, Status, StartType
      } else {
        Write-Host "TinyTop Windows service is not installed."
      }
    }
    default { throw "Unknown service command: $Action" }
  }
}

function Invoke-TinyTopRust {
  param([string[]]$Rest)

  $sub = if ($Rest.Count -gt 0) { $Rest[0] } else { "help" }
  switch ($sub) {
    "install-binary" { Install-TinyTopRustBinary }
    "build" {
      $printCommand = $Rest -contains "--print-command"
      Invoke-TinyTopRustBuild -PrintCommand:$printCommand
    }
    "help" { Write-TinyTopHelp }
    default { throw "Unknown rust command: $sub" }
  }
}

$Command = if ($args.Count -gt 0) { $args[0] } else { "help" }
$Rest = if ($args.Count -gt 1) { $args[1..($args.Count - 1)] } else { @() }

switch ($Command) {
  "help" { Write-TinyTopHelp }
  "-h" { Write-TinyTopHelp }
  "--help" { Write-TinyTopHelp }
  "doctor" { Get-TinyTopStatus }
  "status" { Get-TinyTopStatus }
  "rust" { Invoke-TinyTopRust -Rest $Rest }
  "start" { Start-TinyTop }
  "stop" { Stop-TinyTop }
  "restart" { Restart-TinyTop }
  "logs" { Show-TinyTopLogs }
  "service" {
    $serviceAction = if ($Rest.Count -gt 0) { $Rest[0] } else { "status" }
    Invoke-TinyTopService -Action $serviceAction
  }
  default { throw "Unknown command: $Command" }
}
