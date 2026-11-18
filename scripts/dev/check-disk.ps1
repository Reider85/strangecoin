# check-disk.ps1 - preflight free-space / target-size guard (disk-budget).
#
# Incident 2026-11-18: C: filled to 0 bytes free while target/ sat at
# 615 GB (deps 510 + incremental 54 + build 27); opencode crashed with
# ENOSPC mid-prompt. Run this BEFORE long cargo test/check cycles.
#
# Exit codes:
#   0  - OK (warnings may have been printed)
#   1  - ABORT: free space below -AbortGb (do not start cargo)
#   2  - usage / measurement error
#
# Thresholds (decided 2026-11-18): warn < 100 GB free, abort < 40 GB free.
# NOTE: keep this file ASCII-only - PowerShell 5.1 parses BOM-less .ps1 as
# ANSI and non-ASCII characters break the parser.

[CmdletBinding()]
param(
    [double]$WarnGb = 100,
    [double]$AbortGb = 40,
    [double]$TargetBudgetGb = 150,
    # Skip the (slow, ~30s on 320k files) target/ size walk.
    [switch]$NoTargetSize
)

$ErrorActionPreference = 'Stop'

try {
    $repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
    $drive = (Get-Item -LiteralPath $repoRoot).PSDrive
    if (-not $drive) { throw "cannot resolve drive for $repoRoot" }
    $freeGb = [math]::Round($drive.Free / 1GB, 1)
    $usedGb = [math]::Round($drive.Used / 1GB, 1)
} catch {
    Write-Error "check-disk: $($_.Exception.Message)"
    exit 2
}

Write-Host ("check-disk: drive {0}: used {1} GB, free {2} GB" -f $drive.Name, $usedGb, $freeGb)

$targetDir = Join-Path $repoRoot 'target'
$targetGb = $null
if (-not $NoTargetSize -and (Test-Path -LiteralPath $targetDir)) {
    try {
        $sum = (Get-ChildItem -LiteralPath $targetDir -Recurse -File -Force -ErrorAction SilentlyContinue |
                Measure-Object -Property Length -Sum).Sum
        if ($null -eq $sum) { $sum = 0 }
        $targetGb = [math]::Round($sum / 1GB, 1)
        Write-Host ("check-disk: target/ = {0} GB (budget {1} GB)" -f $targetGb, $TargetBudgetGb)
    } catch {
        Write-Warning "check-disk: target size walk failed: $($_.Exception.Message)"
    }
}

$fail = $false

if ($targetGb -ne $null -and $targetGb -gt $TargetBudgetGb) {
    Write-Warning ("check-disk: target/ is {0} GB over the {1} GB budget - run scripts\dev\prune-target.ps1 -Report" -f $targetGb, $TargetBudgetGb)
}

if ($freeGb -lt $AbortGb) {
    Write-Host ""
    Write-Host ("check-disk: ABORT - free space {0} GB is below the {1} GB floor." -f $freeGb, $AbortGb) -ForegroundColor Red
    Write-Host "  A cold workspace rebuild (rocksdb C++ + 45 test exes) needs tens of GB."
    Write-Host "  Free space first: scripts\dev\prune-target.ps1 -Light   (then -Report)"
    $fail = $true
} elseif ($freeGb -lt $WarnGb) {
    Write-Warning ("check-disk: free space {0} GB is below the {1} GB warning line - prune target before a long test cycle." -f $freeGb, $WarnGb)
}

if ($fail) { exit 1 }
exit 0
