# prune-target.ps1 - safe disk-budget pruning of the strangecoin target/.
#
# Incident 2026-11-18: target/ reached 615 GB (deps 510 + incremental 54 +
# build 27.6 + examples 17.8) and C: hit 0 bytes free. This script removes
# ONLY regenerable caches/logs; it never touches .fingerprint/, deps/ or
# build/ outputs unless -Full is passed explicitly.
#
# Modes:
#   (default) / -Light : incremental caches, *.log, fuzz-typecheck, dlltool-*
#                        shims, soak-logs, tmp  (~55+ GB after an incident)
#   -Report            : size table of target/ top-level dirs + guidance
#   -Full              : cargo clean (drops EVERYTHING; next build = full
#                        cold cycle, rocksdb C++ ~30-40 min - explicit only)
#
# Exit codes: 0 ok, 2 usage/measurement error.
#
# NOTE: keep this file ASCII-only - PowerShell 5.1 parses BOM-less .ps1 as
# ANSI and non-ASCII characters break the parser.

[CmdletBinding(DefaultParameterSetName = 'Light')]
param(
    [Parameter(ParameterSetName = 'Light')][switch]$Light,
    [Parameter(ParameterSetName = 'Report', Mandatory = $true)][switch]$Report,
    [Parameter(ParameterSetName = 'Full', Mandatory = $true)][switch]$Full
)

$ErrorActionPreference = 'Stop'

try {
    $repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
    $targetDir = Join-Path $repoRoot 'target'
} catch {
    Write-Error "prune-target: $($_.Exception.Message)"
    exit 2
}

function Get-DirSizeGb([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) { return 0 }
    return [math]::Round($sum / 1GB, 2)
}

function Remove-TreeSafe([string]$Path) {
    # Never follow junctions/symlinks into foreign trees: PS5.1 Remove-Item
    # -Recurse is reparse-aware, but guard anyway (AGENTS.md C:\rt incident).
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        Write-Warning "prune-target: skip reparse point $Path"
        return $false
    }
    Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
    return $true
}

if ($Report) {
    if (-not (Test-Path -LiteralPath $targetDir)) {
        Write-Host "prune-target: no target/ directory - nothing to report."
        exit 0
    }
    $rows = @(Get-ChildItem -LiteralPath $targetDir -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
        [PSCustomObject]@{ Dir = $_.Name; GB = (Get-DirSizeGb $_.FullName) }
    })
    $rows += @(Get-ChildItem -LiteralPath (Join-Path $targetDir 'debug') -Directory -Force -ErrorAction SilentlyContinue |
        ForEach-Object { [PSCustomObject]@{ Dir = "debug\$($_.Name)"; GB = (Get-DirSizeGb $_.FullName) } })
    $rows | Sort-Object GB -Descending | Format-Table -AutoSize
    $total = Get-DirSizeGb $targetDir
    Write-Host ("prune-target: TOTAL target/ = {0} GB" -f $total)
    $deps = $rows | Where-Object { $_.Dir -eq 'debug\deps' }
    if ($deps -and $deps.GB -gt 150) {
        Write-Host "prune-target: debug\deps is the bulk - stale hashed rlibs/exes from old"
        Write-Host "  fingerprint generations accumulate there. Only 'cargo clean' reclaims it"
        Write-Host "  (full cold rebuild, rocksdb C++ ~30-40 min). Run prune-target.ps1 -Full"
        Write-Host "  only when you can afford that, or after switching env fingerprints."
    }
    exit 0
}

if ($Full) {
    Write-Warning "prune-target: -Full runs cargo clean - the NEXT build is a full cold cycle."
    if (-not (Test-Path -LiteralPath $targetDir)) {
        Write-Host "prune-target: no target/ directory."
        exit 0
    }
    $before = Get-DirSizeGb $targetDir
    Remove-TreeSafe $targetDir | Out-Null
    Write-Host ("prune-target: removed target/ ({0} GB freed)." -f $before)
    exit 0
}

# --- Light (default) -------------------------------------------------------
if (-not (Test-Path -LiteralPath $targetDir)) {
    Write-Host "prune-target: no target/ directory - nothing to prune."
    exit 0
}

$targets = @(
    (Join-Path $targetDir 'debug\incremental'),
    (Join-Path $targetDir 'fuzz-typecheck'),
    (Join-Path $targetDir 'dlltool-shim'),
    (Join-Path $targetDir 'dlltool-test'),
    (Join-Path $targetDir 'dlltool-test2'),
    (Join-Path $targetDir 'soak-logs'),
    (Join-Path $targetDir 'tmp'),
    (Join-Path $targetDir 'gh-bash'),
    (Join-Path $targetDir 'mig')
)

$freed = 0.0
foreach ($t in $targets) {
    if (Test-Path -LiteralPath $t) {
        $gb = Get-DirSizeGb $t
        if (Remove-TreeSafe $t) {
            Write-Host ("prune-target: removed {0} ({1} GB)" -f (Split-Path $t -Leaf), $gb)
            $freed += $gb
        }
    }
}

# Stray build logs at target/ root (s3p15c_*.log, mcp_*.log, cargo_test_*.log, ...).
$logFiles = @(Get-ChildItem -LiteralPath $targetDir -File -Force -ErrorAction SilentlyContinue |
    Where-Object { $_.Extension -eq '.log' -or $_.Name -match '\.exit$' })
foreach ($f in $logFiles) {
    $freed += ($f.Length / 1GB)
    Remove-Item -LiteralPath $f.FullName -Force -ErrorAction SilentlyContinue
}
if ($logFiles.Count -gt 0) {
    Write-Host ("prune-target: removed {0} root log file(s)" -f $logFiles.Count)
}

Write-Host ("prune-target: light prune freed ~{0} GB." -f [math]::Round($freed, 2))
Write-Host "prune-target: run with -Report for a full size table."
exit 0
