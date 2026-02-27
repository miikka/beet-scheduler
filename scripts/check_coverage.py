#!/usr/bin/env python3
"""
Compare cargo llvm-cov --json output against a saved baseline.

Usage:
    # Save current coverage as baseline
    cargo llvm-cov --json | python3 scripts/check_coverage.py --save-baseline

    # Compare against baseline (whole repo)
    cargo llvm-cov --json | python3 scripts/check_coverage.py

    # Compare specific files only (in addition to totals)
    cargo llvm-cov --json | python3 scripts/check_coverage.py src/config.rs src/state.rs

    # Read coverage from a file instead of stdin
    cargo llvm-cov --json > cov.json
    python3 scripts/check_coverage.py --input cov.json
"""

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_BASELINE = f".config/coverage-baseline-{sys.platform}.json"


def parse_args():
    parser = argparse.ArgumentParser(
        description="Compare cargo llvm-cov line coverage against a baseline.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "--input", "-i",
        metavar="FILE",
        help="Read JSON coverage from FILE instead of stdin",
    )
    parser.add_argument(
        "--baseline", "-b",
        metavar="FILE",
        default=DEFAULT_BASELINE,
        help=f"Path to baseline file (default: {DEFAULT_BASELINE})",
    )
    parser.add_argument(
        "--save-baseline",
        action="store_true",
        help="Save current coverage as the new baseline and exit",
    )
    parser.add_argument(
        "--tolerance",
        metavar="PCT",
        type=float,
        default=0.0,
        help="Allow coverage to drop by up to PCT%% before failing (default: 0.0)",
    )
    parser.add_argument(
        "files",
        nargs="*",
        metavar="FILE",
        help="Source files to check individually (relative or absolute paths)",
    )
    return parser.parse_args()


def load_coverage(source):
    """Load and parse cargo llvm-cov --json output."""
    try:
        data = json.load(source)
    except json.JSONDecodeError as e:
        print(f"error: failed to parse coverage JSON: {e}", file=sys.stderr)
        sys.exit(1)

    if "data" not in data or not data["data"]:
        print("error: unexpected llvm-cov JSON structure (missing 'data')", file=sys.stderr)
        sys.exit(1)

    report = data["data"][0]
    return report


def extract_line_pct(summary):
    lines = summary["lines"]
    return lines["percent"], lines["covered"], lines["count"]


def normalize_path(path, project_root):
    """Return a project-relative path like 'src/config.rs'."""
    try:
        return str(Path(path).relative_to(project_root))
    except ValueError:
        return path


def build_coverage_snapshot(report, project_root):
    """Build a dict with total and per-file line coverage percentages."""
    totals = report["totals"]
    total_pct, total_covered, total_count = extract_line_pct(totals)

    files = {}
    for f in report["files"]:
        rel = normalize_path(f["filename"], project_root)
        pct, covered, count = extract_line_pct(f["summary"])
        files[rel] = {"percent": pct, "covered": covered, "count": count}

    return {
        "created": datetime.now(timezone.utc).isoformat(),
        "total": {"percent": total_pct, "covered": total_covered, "count": total_count},
        "files": files,
    }


def save_baseline(snapshot, path):
    with open(path, "w") as f:
        json.dump(snapshot, f, indent=2)
        f.write("\n")
    print(f"Baseline saved to {path}")
    print(f"  Total line coverage: {snapshot['total']['percent']:.2f}%  "
          f"({snapshot['total']['covered']}/{snapshot['total']['count']} lines)")


def load_baseline(path):
    if not os.path.exists(path):
        print(f"error: baseline file not found: {path}", file=sys.stderr)
        print("hint: run with --save-baseline to create one", file=sys.stderr)
        sys.exit(1)
    with open(path) as f:
        return json.load(f)


def delta_str(current_pct, baseline_pct):
    diff = current_pct - baseline_pct
    sign = "+" if diff >= 0 else ""
    return f"{sign}{diff:.2f}%"


def status_symbol(passed):
    return "OK" if passed else "FAIL"


def compare_coverage(current, baseline, files_to_check, tolerance, project_root):
    """
    Compare current coverage against baseline.
    Returns True if all checks pass (no regression beyond tolerance).
    """
    failures = []

    # --- Totals ---
    cur_total = current["total"]
    bas_total = baseline.get("total", {})
    bas_total_pct = bas_total.get("percent", 0.0)
    cur_total_pct = cur_total["percent"]
    total_ok = cur_total_pct >= bas_total_pct - tolerance

    print("Overall line coverage")
    print(f"  Current:  {cur_total_pct:.2f}%  ({cur_total['covered']}/{cur_total['count']} lines)")
    print(f"  Baseline: {bas_total_pct:.2f}%")
    print(f"  Delta:    {delta_str(cur_total_pct, bas_total_pct)}  [{status_symbol(total_ok)}]")

    if not total_ok:
        failures.append(
            f"Total line coverage dropped: {cur_total_pct:.2f}% vs baseline {bas_total_pct:.2f}%"
        )

    # --- Per-file checks ---
    if files_to_check:
        print()
        print("Per-file line coverage")

        for raw_path in files_to_check:
            # Normalize: try relative to project root, then as-is
            abs_path = Path(raw_path)
            if not abs_path.is_absolute():
                abs_path = project_root / raw_path
            rel = normalize_path(str(abs_path), project_root)

            cur_file = current["files"].get(rel)
            bas_file = baseline.get("files", {}).get(rel)

            if cur_file is None:
                print(f"  {rel}: not found in current coverage report")
                failures.append(f"{rel}: not found in current coverage report")
                continue

            cur_pct = cur_file["percent"]
            bas_pct = bas_file["percent"] if bas_file else None

            if bas_pct is None:
                print(f"  {rel}: {cur_pct:.2f}%  ({cur_file['covered']}/{cur_file['count']})  [no baseline]")
            else:
                ok = cur_pct >= bas_pct - tolerance
                print(
                    f"  {rel}: {cur_pct:.2f}%  ({cur_file['covered']}/{cur_file['count']})  "
                    f"baseline {bas_pct:.2f}%  delta {delta_str(cur_pct, bas_pct)}  [{status_symbol(ok)}]"
                )
                if not ok:
                    failures.append(
                        f"{rel}: line coverage dropped: {cur_pct:.2f}% vs baseline {bas_pct:.2f}%"
                    )

    # --- Summary ---
    if failures:
        print()
        print("FAILED — coverage regressions detected:")
        for msg in failures:
            print(f"  - {msg}")
        if tolerance > 0:
            print(f"  (tolerance: {tolerance:.2f}%)")
        return False

    print()
    print("PASSED — no coverage regressions")
    return True


def main():
    args = parse_args()
    project_root = Path.cwd()

    # Load coverage data
    if args.input:
        with open(args.input) as f:
            report = load_coverage(f)
    else:
        if sys.stdin.isatty():
            print("error: no input provided — pipe cargo llvm-cov --json or use --input", file=sys.stderr)
            sys.exit(1)
        report = load_coverage(sys.stdin)

    snapshot = build_coverage_snapshot(report, project_root)

    if args.save_baseline:
        save_baseline(snapshot, args.baseline)
        return

    baseline = load_baseline(args.baseline)

    passed = compare_coverage(
        current=snapshot,
        baseline=baseline,
        files_to_check=args.files,
        tolerance=args.tolerance,
        project_root=project_root,
    )

    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
