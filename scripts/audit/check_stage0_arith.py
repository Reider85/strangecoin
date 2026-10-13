#!/usr/bin/env python3
"""BUG-S1-014: Stage 0 summary arithmetic consistency gate.

Verifies that the three Stage 0 summary documents agree with their own
detail tables and with each other:

  docs/stage0/INVARIANTS_ENFORCED.md
      summary counts == section row counts; sum == 22; invariants 1..22
      covered exactly once (no duplicates, no gaps).
  docs/stage0/CRITICAL_ISSUES_CLOSED.md
      header status line == Summary table == Detailed Status tallies.
  docs/stage0/STAGE0_SUMMARY.md
      Overview metric rows match the two documents above.

Paths resolve relative to the repository root, so the script may be run
from anywhere. Exit 0 = consistent; exit 1 = mismatch (report on stdout).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INVARIANTS_MD = ROOT / "docs" / "stage0" / "INVARIANTS_ENFORCED.md"
CRITICAL_MD = ROOT / "docs" / "stage0" / "CRITICAL_ISSUES_CLOSED.md"
SUMMARY_MD = ROOT / "docs" / "stage0" / "STAGE0_SUMMARY.md"

TOTAL_INVARIANTS = 22
ERRORS: list[str] = []


def err(msg: str) -> None:
    ERRORS.append(msg)


def section_rows(text: str, header: str) -> list[str]:
    """Return numeric table rows between '## <header>' and the next '## '."""
    m = re.search(rf"^## {re.escape(header)}\s*$(.*?)(?=^## |\Z)", text, re.M | re.S)
    if not m:
        err(f"INVARIANTS: missing section '## {header}'")
        return []
    return [
        ln for ln in m.group(1).splitlines() if re.match(r"^\|\s*\d+\s*\|", ln)
    ]


def check_invariants() -> dict[str, int] | None:
    base = len(ERRORS)
    text = INVARIANTS_MD.read_text(encoding="utf-8")
    summary: dict[str, int] = {}
    for label, key in [
        ("Enforced", "enforced"),
        ("Deferred", "deferred"),
        ("N/A at Stage 0", "na"),
    ]:
        m = re.search(rf"^\|\s*{re.escape(label)}\s*\|\s*(\d+)\s*\|\s*$", text, re.M)
        if not m:
            err(f"INVARIANTS: summary row '{label}' not found")
        else:
            summary[key] = int(m.group(1))
    actual = {
        "enforced": len(section_rows(text, "Enforced Invariants")),
        "deferred": len(section_rows(text, "Deferred Invariants")),
        "na": len(section_rows(text, "N/A at Stage 0")),
    }
    for key, label in [
        ("enforced", "Enforced"),
        ("deferred", "Deferred"),
        ("na", "N/A at Stage 0"),
    ]:
        if key in summary and summary[key] != actual[key]:
            err(
                f"INVARIANTS: summary says {label}={summary[key]}, "
                f"table has {actual[key]} rows"
            )
    total = sum(actual.values())
    if total != TOTAL_INVARIANTS:
        err(f"INVARIANTS: section rows sum to {total}, expected {TOTAL_INVARIANTS}")
    m = re.search(r"^\|\s*\*\*Total\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*$", text, re.M)
    if m and int(m.group(1)) != total:
        err(f"INVARIANTS: Total row says {m.group(1)}, sections sum to {total}")
    nums = [
        int(re.match(r"^\|\s*(\d+)", ln).group(1))
        for ln in (
            section_rows(text, "Enforced Invariants")
            + section_rows(text, "Deferred Invariants")
            + section_rows(text, "N/A at Stage 0")
        )
    ]
    dupes = sorted({n for n in nums if nums.count(n) > 1})
    if dupes:
        err(f"INVARIANTS: duplicate invariant numbers {dupes}")
    missing = sorted(set(range(1, TOTAL_INVARIANTS + 1)) - set(nums))
    if missing:
        err(f"INVARIANTS: missing invariant numbers {missing}")
    return summary if len(ERRORS) == base else None


def check_critical() -> dict[str, int] | None:
    base = len(ERRORS)
    text = CRITICAL_MD.read_text(encoding="utf-8")
    header: dict[str, int] = {}
    m = re.search(
        r"\*\*Status:\*\*\s*(\d+)\s*closed,\s*(\d+)\s*partially closed,"
        r"\s*(\d+)\s*deferred",
        text,
    )
    if not m:
        err(
            "CRITICAL: header '**Status:** N closed, N partially closed, "
            "N deferred' not found"
        )
    else:
        header = {
            "closed": int(m.group(1)),
            "partial": int(m.group(2)),
            "deferred": int(m.group(3)),
        }
    summary: dict[str, int] = {}
    for label, key in [
        ("Closed", "closed"),
        ("Partially closed", "partial"),
        ("Deferred", "deferred"),
    ]:
        m = re.search(rf"^\|\s*{re.escape(label)}\s*\|\s*(\d+)\s*\|\s*$", text, re.M)
        if not m:
            err(f"CRITICAL: summary row '{label}' not found")
        else:
            summary[key] = int(m.group(1))
    detail = {"closed": 0, "partial": 0, "deferred": 0}
    in_detail = False
    for ln in text.splitlines():
        if ln.startswith("## Detailed Status"):
            in_detail = True
            continue
        if in_detail and ln.startswith("## "):
            break
        if not in_detail or not ln.startswith("|"):
            continue
        if "**Closed**" in ln:
            detail["closed"] += 1
        elif "**Partially closed**" in ln:
            detail["partial"] += 1
        elif "**Deferred**" in ln:
            detail["deferred"] += 1
    for key, label in [
        ("closed", "Closed"),
        ("partial", "Partially closed"),
        ("deferred", "Deferred"),
    ]:
        if key in header and header[key] != detail[key]:
            err(
                f"CRITICAL: header says {label}={header[key]}, "
                f"detail table has {detail[key]}"
            )
        if key in summary and summary[key] != detail[key]:
            err(
                f"CRITICAL: summary says {label}={summary[key]}, "
                f"detail table has {detail[key]}"
            )
    return detail if len(ERRORS) == base else None


def check_summary(
    inv: dict[str, int] | None, crit: dict[str, int] | None
) -> None:
    text = SUMMARY_MD.read_text(encoding="utf-8")
    m = re.search(
        r"^\|\s*Critical issues closed\s*\|\s*(\d+)\s*closed,"
        r"\s*(\d+)\s*partially closed,\s*(\d+)\s*deferred",
        text,
        re.M,
    )
    if not m:
        err("STAGE0_SUMMARY: 'Critical issues closed' metric row not found")
    elif crit:
        got = (int(m.group(1)), int(m.group(2)), int(m.group(3)))
        want = (crit["closed"], crit["partial"], crit["deferred"])
        if got != want:
            err(
                f"STAGE0_SUMMARY: critical issues {got} != "
                f"CRITICAL_ISSUES_CLOSED {want}"
            )
    m = re.search(
        r"^\|\s*Invariants enforced\s*\|\s*(\d+)/22\s+enforced,"
        r"\s*(\d+)\s+deferred,\s*(\d+)\s+N/A",
        text,
        re.M,
    )
    if not m:
        err("STAGE0_SUMMARY: 'Invariants enforced' metric row not found")
    elif inv:
        got = (int(m.group(1)), int(m.group(2)), int(m.group(3)))
        want = (inv.get("enforced"), inv.get("deferred"), inv.get("na"))
        if got != want:
            err(
                f"STAGE0_SUMMARY: invariants {got} != "
                f"INVARIANTS_ENFORCED {want}"
            )


def report() -> None:
    print("Stage 0 summary arithmetic mismatch (BUG-S1-014):")
    for e in ERRORS:
        print(f"  - {e}")


def main() -> int:
    for path in (INVARIANTS_MD, CRITICAL_MD, SUMMARY_MD):
        if not path.is_file():
            err(f"missing file: {path.relative_to(ROOT)}")
    if ERRORS:
        report()
        return 1
    inv = check_invariants()
    crit = check_critical()
    check_summary(inv, crit)
    if ERRORS:
        report()
        return 1
    print("OK: Stage 0 summary arithmetic is consistent (BUG-S1-014)")
    print(f"  INVARIANTS_ENFORCED: {inv}")
    print(f"  CRITICAL_ISSUES_CLOSED: {crit}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
