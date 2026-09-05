#!/usr/bin/env python3
"""Render a Markdown coverage table from `cargo llvm-cov report --json`.

Usage: coverage_summary.py HEAD.json [BASE.json]

Rows are grouped per workspace crate (derived from the source path), with the
line-coverage delta against BASE when given. Files under `target/` and
`examples/`/`tests/` fixtures are still counted (they are part of the
workspace) but examples are listed separately so library crates stay visible.
"""
import json
import os
import re
import sys
from collections import defaultdict

CRATE_RE = re.compile(
    r"(?:^|/)(crates/rpc/bidirectional/[^/]+|crates/[^/]+/[^/]+|examples/[^/]+(?:/[^/]+)?|tests/playwright/fixtures/[^/]+)/"
)


def load(path):
    if not path or not os.path.exists(path):
        return None
    with open(path) as f:
        data = json.load(f)
    return data.get("data", [None])[0]


def per_crate(report):
    """-> {crate: (covered_lines, total_lines)}"""
    out = defaultdict(lambda: [0, 0])
    if not report:
        return out
    for f in report.get("files", []):
        m = CRATE_RE.search(f["filename"])
        crate = m.group(1) if m else "(other)"
        lines = f["summary"]["lines"]
        out[crate][0] += lines["covered"]
        out[crate][1] += lines["count"]
    return out


def pct(cov, tot):
    return 100.0 * cov / tot if tot else 0.0


def fmt_delta(d):
    if d is None:
        return ""
    if abs(d) < 0.005:
        return "±0.00"
    return f"{d:+.2f}"


def main():
    head = load(sys.argv[1])
    base = load(sys.argv[2]) if len(sys.argv) > 2 else None
    if head is None:
        print("Coverage report unavailable.")
        return

    hc = per_crate(head)
    bc = per_crate(base) if base else None

    total_lines = head["totals"]["lines"]
    total_pct = total_lines["percent"]
    total_delta = None
    if base:
        total_delta = total_pct - base["totals"]["lines"]["percent"]

    print("### Test coverage (lines)")
    print()
    headline = f"**Total: {total_pct:.2f}%**"
    if total_delta is not None:
        headline += f" ({fmt_delta(total_delta)} vs base)"
    print(headline, f"— {total_lines['covered']}/{total_lines['count']} lines")
    print()
    cols = "| Crate | Lines | Coverage |" + (" Δ |" if base else "")
    print(cols)
    print("|---|---:|---:|" + ("---:|" if base else ""))

    def rows(prefix):
        for crate in sorted(k for k in hc if k.startswith(prefix)):
            cov, tot = hc[crate]
            p = pct(cov, tot)
            line = f"| `{crate}` | {cov}/{tot} | {p:.2f}% |"
            if base:
                if crate in bc and bc[crate][1]:
                    d = p - pct(*bc[crate])
                    line += f" {fmt_delta(d)} |"
                else:
                    line += " new |"
            print(line)

    rows("crates/")
    if any(k.startswith(("examples/", "tests/")) for k in hc):
        print("| **Examples and fixtures** | | |" + (" |" if base else ""))
        rows("examples/")
        rows("tests/")
    if "(other)" in hc:
        rows("(other)")

    # Files whose coverage dropped the most, to make regressions actionable.
    if base:
        base_files = {f["filename"]: f["summary"]["lines"] for f in base.get("files", [])}
        drops = []
        for f in head.get("files", []):
            b = base_files.get(f["filename"])
            if not b or not b["count"]:
                continue
            d = f["summary"]["lines"]["percent"] - b["percent"]
            if d <= -1.0:
                drops.append((d, f["filename"], f["summary"]["lines"]["percent"]))
        if drops:
            print()
            print("<details><summary>Files with coverage drops ≥ 1 point</summary>")
            print()
            print("| File | Coverage | Δ |")
            print("|---|---:|---:|")
            for d, name, p in sorted(drops)[:25]:
                short = name.split("/rust-api-stack/", 1)[-1]
                print(f"| `{short}` | {p:.2f}% | {fmt_delta(d)} |")
            print()
            print("</details>")


if __name__ == "__main__":
    main()
