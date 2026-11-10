#Requires -Version 5.1
<#
.SYNOPSIS
  One-time setup of the local self-hosted GitHub Actions runner used by the
  hybrid-runner fallback (analytics/GITHUB.md P6, .github/SELF_HOSTED_RUNNERS.md).

.DESCRIPTION
  Idempotent. Prepares this Windows host to execute strangecoin CI jobs when
  the repo variable RUNS_ON points at the local runner:

    1. Installs runner prerequisites via winget (ripgrep, jq, GitHub CLI,
       Python 3.12) - needed by ci.yml source-gates and the star/prerelease
       bash steps; skipped with -SkipTools if already provisioned manually.
    2. Machine-level environment (survives service start / new terminals):
       - prepends C:\msys64\mingw64\bin to the machine PATH (dlltool/as/gcc
         for the windows-gnu toolchain - AGENTS.md Windows preamble);
       - sets CARGO_HOME=C:\rt\cargo_home (ASCII path, AGENTS.md);
       - creates C:\rt\cargo_home\config.toml with the windows-gnu linker +
         --sysroot pins if absent (the repo's .cargo/config.toml is
         gitignored and actions/checkout would wipe it - the CARGO_HOME
         copy survives every checkout).
    3. git config --global core.autocrlf false (LF-only CSV/docs parity
       with the ubuntu runners).
    4. rustup: installs stable-x86_64-pc-windows-gnu if missing (the
       toolchain ci.yml pins while RUNS_ON is set).
    5. Downloads + registers actions/runner into an ASCII path
       (C:\actions-runner by default - the Windows user profile is
       Cyrillic, which breaks GNU ld / some runner tooling).
    6. Prints the follow-up manual steps (start runner, set RUNS_ON).

.PARAMETER Token
  Registration token from GitHub -> Settings -> Actions -> Runners ->
  "New self-hosted runner" (button "Configure runner" shows
  `config.cmd --url ... --token ...`). Short-lived; re-issue if expired.

.PARAMETER RepoUrl
  Repository URL the runner registers against.

.PARAMETER RunnerRoot
  Install directory for actions/runner. Must be a pure-ASCII path.

.PARAMETER InstallService
  Also register the runner as a Windows service (requires an elevated
  shell). Without it the runner is configured only; start it manually
  with run.cmd (or re-run elevated with -InstallService later).

.PARAMETER SkipTools
  Skip the winget prerequisite installs (tools already present).

.EXAMPLE
  .\scripts\actions\setup-local-runner.ps1 -Token <registration-token>
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Token,

    [string]$RepoUrl = 'https://github.com/Reider85/strangecoin',

    [string]$RunnerRoot = 'C:\actions-runner',

    [switch]$InstallService,

    [switch]$SkipTools
)

$ErrorActionPreference = 'Stop'

# PS 5.1 defaults to TLS 1.0 on some hosts - GitHub API/CDN require TLS 1.2.
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch { }

function Write-Step([string]$Message) {
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Info([string]$Message) {
    Write-Host "    $Message"
}

# ---------------------------------------------------------------------------
# 0. Sanity checks
# ---------------------------------------------------------------------------
if ($RunnerRoot -notmatch '^[A-Za-z0-9_\\:\.\-]+$') {
    throw "RunnerRoot must be a pure-ASCII path (got: $RunnerRoot). The Windows profile contains non-ASCII characters that break GNU ld and parts of the runner tooling."
}

Write-Step 'Step 0/7: sanity checks'

# ---------------------------------------------------------------------------
# 1. Prerequisites via winget (ripgrep, jq, gh, python)
# ---------------------------------------------------------------------------
if (-not $SkipTools) {
    Write-Step 'Step 1/7: prerequisites (winget)'

    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if (-not $winget) {
        Write-Warning 'winget not found - install "App Installer" from the Microsoft Store, then re-run, or install manually: ripgrep, jq, GitHub CLI, Python 3.12 (all must end up on the machine PATH).'
    } else {
        $tools = @(
            @{ Id = 'BurntSushi.ripgrep.MSVC'; Probe = 'rg' },
            @{ Id = 'jqlang.jq';              Probe = 'jq' },
            @{ Id = 'GitHub.cli';             Probe = 'gh' },
            @{ Id = 'Python.Python.3.12';     Probe = 'python' }
        )
        foreach ($tool in $tools) {
            $found = Get-Command $tool.Probe -ErrorAction SilentlyContinue
            if ($found) {
                Write-Info "$($tool.Probe): already on PATH ($($found.Source))"
                continue
            }
            Write-Info "installing $($tool.Id) ..."
            & winget install --id $tool.Id -e --accept-source-agreements --accept-package-agreements --disable-interactivity | Out-Null
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "winget install $($tool.Id) exited with $LASTEXITCODE - verify '$($tool.Probe)' ends up on the machine PATH before the first CI run."
            }
        }
    }
} else {
    Write-Step 'Step 1/7: prerequisites skipped (-SkipTools)'
}

# Refresh PATH in *this* process so the probes below see freshly installed
# tools (machine PATH changes only apply to new processes).
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$env:Path = "$machinePath;$userPath"

# ---------------------------------------------------------------------------
# 2. Machine-level environment: mingw64 PATH, CARGO_HOME, cargo config
# ---------------------------------------------------------------------------
Write-Step 'Step 2/7: machine-level environment (PATH, CARGO_HOME, cargo config)'

$mingw = 'C:\msys64\mingw64\bin'
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
if ($machinePath -notlike "*$mingw*") {
    [Environment]::SetEnvironmentVariable('Path', "$mingw;$machinePath", 'Machine')
    Write-Info "prepended $mingw to the machine PATH"
} else {
    Write-Info "$mingw already on the machine PATH"
}
$env:Path = "$mingw;$env:Path"

$cargoHome = 'C:\rt\cargo_home'
$existingCargoHome = [Environment]::GetEnvironmentVariable('CARGO_HOME', 'Machine')
if ($existingCargoHome -ne $cargoHome) {
    [Environment]::SetEnvironmentVariable('CARGO_HOME', $cargoHome, 'Machine')
    Write-Info "machine CARGO_HOME=$cargoHome (was: '$existingCargoHome')"
} else {
    Write-Info "machine CARGO_HOME already $cargoHome"
}
$env:CARGO_HOME = $cargoHome

# The junction (C:\rt\cargo_home -> %USERPROFILE%\.cargo) from AGENTS.md:
# recreate it when missing so CARGO_HOME and the interactive dev environment
# share one registry cache.
if (-not (Test-Path $cargoHome)) {
    $userCargo = Join-Path $env:USERPROFILE '.cargo'
    if (-not (Test-Path $userCargo)) {
        New-Item -ItemType Directory -Path $userCargo | Out-Null
    }
    try {
        New-Item -ItemType Junction -Path $cargoHome -Target $userCargo | Out-Null
        Write-Info "created junction $cargoHome -> $userCargo"
    } catch {
        New-Item -ItemType Directory -Path $cargoHome | Out-Null
        Write-Info "created directory $cargoHome (junction failed: $($_.Exception.Message))"
    }
} else {
    Write-Info "$cargoHome already exists"
}

# CARGO_HOME-level cargo config: the repo's .cargo/config.toml is gitignored,
# so actions/checkout never delivers it to the runner work dir - without
# these pins the windows-gnu link fails on the Cyrillic profile paths
# (AGENTS.md "windows-gnu linking Stage 3").
$cargoConfig = Join-Path $cargoHome 'config.toml'
$gnuConfig = @'

[target.x86_64-pc-windows-gnu]
linker = "gcc"
rustflags = ["--sysroot", "C:\\rt\\toolchain"]
'@
if (-not (Test-Path $cargoConfig)) {
    Set-Content -Path $cargoConfig -Value $gnuConfig.Trim() -Encoding Ascii
    Write-Info "wrote $cargoConfig (windows-gnu linker + sysroot pins)"
} else {
    $current = Get-Content -Path $cargoConfig -Raw
    if ($current -notmatch 'target\.x86_64-pc-windows-gnu') {
        Add-Content -Path $cargoConfig -Value $gnuConfig -Encoding Ascii
        Write-Info "appended [target.x86_64-pc-windows-gnu] section to $cargoConfig"
    } else {
        Write-Info "$cargoConfig already pins the windows-gnu target"
    }
}

# LF-only working trees (stargazer-history.csv is asserted LF-only; the
# ubuntu runners never convert). EAP-safe: native stderr redirection under
# $ErrorActionPreference='Stop' can throw in PS 5.1 - probe without it.
$autocrlf = $null
git config --global core.autocrlf > $null 2>&1
if ($LASTEXITCODE -eq 0) {
    $autocrlf = (& git config --global core.autocrlf) -join ''
}
if ($autocrlf -ne 'false') {
    git config --global core.autocrlf false
    if ($LASTEXITCODE -ne 0) { Write-Warning 'git config core.autocrlf=false failed - set it manually.' }
    else { Write-Info 'git config --global core.autocrlf false' }
} else {
    Write-Info 'git core.autocrlf already false'
}

# ---------------------------------------------------------------------------
# 3. Toolchain
# ---------------------------------------------------------------------------
Write-Step 'Step 3/7: rustup toolchain (stable-x86_64-pc-windows-gnu)'

$rustup = Get-Command rustup -ErrorAction SilentlyContinue
if (-not $rustup) {
    throw 'rustup not found on PATH - install Rust via https://rustup.rs first (this host already has it; a fresh host needs it before the runner can build).'
}
$installed = (rustup toolchain list) -join "`n"
if ($installed -match 'stable-x86_64-pc-windows-gnu') {
    Write-Info 'stable-x86_64-pc-windows-gnu already installed'
} else {
    Write-Info 'installing stable-x86_64-pc-windows-gnu (first run downloads ~200 MB)...'
    & rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
    if ($LASTEXITCODE -ne 0) { throw "rustup toolchain install failed (exit $LASTEXITCODE)" }
}

# ---------------------------------------------------------------------------
# 4. Download actions/runner (ASCII root)
# ---------------------------------------------------------------------------
Write-Step 'Step 4/7: download actions/runner'

if (-not (Test-Path $RunnerRoot)) {
    New-Item -ItemType Directory -Path $RunnerRoot | Out-Null
}

$runnerConfigMarker = Join-Path $RunnerRoot '.runner'
$needDownload = -not (Test-Path (Join-Path $RunnerRoot 'config.cmd'))
if (-not $needDownload) {
    Write-Info "runner binaries already present in $RunnerRoot"
} else {
    Write-Info 'querying latest actions/runner release ...'
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/actions/runner/releases/latest' -Headers @{ Accept = 'application/vnd.github+json' }
    $version = $release.tag_name.TrimStart('v')
    $zipName = "actions-runner-win-x64-$version.zip"
    $zipUrl = "https://github.com/actions/runner/releases/download/$($release.tag_name)/$zipName"
    $zipPath = Join-Path $env:TEMP $zipName
    Write-Info "downloading $zipUrl"
    Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath

    Write-Info "extracting to $RunnerRoot ..."
    $extractRoot = Join-Path $env:TEMP "actions-runner-extract-$version"
    if (Test-Path $extractRoot) { Remove-Item -Recurse -Force $extractRoot }
    Expand-Archive -Path $zipPath -DestinationPath $extractRoot -Force
    # The zip nests everything under actions-runner-win-x64-<ver>/.
    $nested = Get-ChildItem -Path $extractRoot -Directory | Select-Object -First 1
    $payload = if ($nested) { $nested.FullName } else { $extractRoot }
    Get-ChildItem -Path $payload | ForEach-Object {
        Move-Item -Path $_.FullName -Destination $RunnerRoot -Force
    }
    Remove-Item -Recurse -Force $extractRoot
    Remove-Item -Force $zipPath
    Write-Info "runner $version extracted"
}

# ---------------------------------------------------------------------------
# 5. Register the runner
# ---------------------------------------------------------------------------
Write-Step 'Step 5/7: register runner'

if (Test-Path $runnerConfigMarker) {
    Write-Info "already registered ($runnerConfigMarker exists) - delete $RunnerRoot and re-run to re-register with a fresh token."
} else {
    # No --labels: keep the platform defaults (self-hosted, Windows, X64)
    # so the RUNS_ON value ["self-hosted","Windows","X64"] matches exactly.
    & (Join-Path $RunnerRoot 'config.cmd') --url $RepoUrl --token $Token --unattended
    if ($LASTEXITCODE -ne 0) { throw "runner config failed (exit $LASTEXITCODE) - the token may be expired; issue a fresh one and re-run." }
    Write-Info 'runner registered'
}

# ---------------------------------------------------------------------------
# 6. Service (optional) / manual start
# ---------------------------------------------------------------------------
Write-Step 'Step 6/7: runner start mode'

if ($InstallService) {
    $elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $elevated) {
        Write-Warning '-InstallService needs an elevated shell. Re-run from an Administrator PowerShell, or start the runner manually (step 7).'
    } else {
        & (Join-Path $RunnerRoot 'svcinstall.cmd')
        Write-Info 'runner installed as a Windows service (starts automatically with Windows)'
    }
} else {
    Write-Info 'not installing a service - start the runner manually (step 7) or re-run elevated with -InstallService.'
}

# ---------------------------------------------------------------------------
# 7. Follow-up instructions
# ---------------------------------------------------------------------------
Write-Step 'Step 7/7: follow-up'

$runCmd = Join-Path $RunnerRoot 'run.cmd'
Write-Host @"

-----------------------------------------------------------------------
Local self-hosted runner ready.

A) Start the runner (skip if you installed the service):
     & "$runCmd"
   Leave that window open while jobs should run (or use -InstallService).

B) Verify it appears online:
     gh api repos/Reider85/strangecoin/actions/runners --jq '.runners[].name'

C) Activate the hybrid fallback (when GitHub free minutes run out):
     gh variable set RUNS_ON --body '["self-hosted","Windows","X64"]'
   Trigger a test:  gh workflow run CI --ref master
   Watch the job pick the local runner, then either keep the variable or
   restore GitHub-hosted execution:
     gh variable delete RUNS_ON

D) Revert after minutes reset - delete RUNS_ON (see C).

Details, degraded-mode matrix and troubleshooting:
     .github/SELF_HOSTED_RUNNERS.md
-----------------------------------------------------------------------
"@
