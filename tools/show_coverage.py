#!/usr/bin/env python3
"""Parse the most recent bazel coverage LCOV report and serve it as HTML.

Usage: bazel run //tools:show_coverage
       (run after: bazel coverage //...)
"""

import functools
import html
import http.server
import os
import pathlib
import socketserver
import sys
import tempfile

WORKSPACE = os.environ.get("BUILD_WORKSPACE_DIRECTORY", os.getcwd())
# bazel-out is a symlink created by Bazel in the workspace root.
REPORT = os.path.join(WORKSPACE, "bazel-out", "_coverage", "_coverage_report.dat")
PORT = 8000

# Only show files from our own crates.
_INCLUDE_PREFIXES = ("services/", "lib/rust/")

CSS = """
body { font-family: monospace; background: #fff; color: #000; margin: 2em; }
h1, h2 { font-family: sans-serif; }
table { border-collapse: collapse; width: 100%; }
th, td { padding: 4px 8px; border: 1px solid #ccc; text-align: left; }
th { background: #eee; }
.hi  { color: #060; font-weight: bold; }
.med { color: #960; font-weight: bold; }
.lo  { color: #c00; font-weight: bold; }
.cov   { background: #dfd; }
.uncov { background: #fdd; }
td.src { white-space: pre; padding-left: 1em; }
a { text-decoration: none; color: inherit; }
a:hover { text-decoration: underline; }
"""


def parse_lcov(path):
    """Return {sf_path: {line_no: hit_count}} from an LCOV .dat file."""
    records: dict[str, dict[int, int]] = {}
    current: str | None = None
    try:
        with open(path) as f:
            for raw in f:
                line = raw.rstrip()
                if line.startswith("SF:"):
                    current = line[3:]
                    records.setdefault(current, {})
                elif line.startswith("DA:") and current is not None:
                    parts = line[3:].split(",")
                    if len(parts) >= 2:
                        try:
                            lineno, hits = int(parts[0]), int(parts[1])
                            records[current][lineno] = (
                                records[current].get(lineno, 0) + hits
                            )
                        except ValueError:
                            pass
                elif line == "end_of_record":
                    current = None
    except FileNotFoundError:
        return {}
    return records


def pct_class(pct: float) -> str:
    if pct >= 80:
        return "hi"
    if pct >= 50:
        return "med"
    return "lo"


def coverage_pct(covered: int, total: int) -> float:
    return 100.0 * covered / total if total else 100.0


def file_html(rel: str, line_hits: dict[int, int]) -> str:
    src_path = os.path.join(WORKSPACE, rel)
    rows = []
    try:
        with open(src_path) as f:
            for i, src_line in enumerate(f, 1):
                hits = line_hits.get(i)
                if hits is None:
                    cls, count = "", ""
                elif hits == 0:
                    cls, count = "uncov", "0"
                else:
                    cls, count = "cov", str(hits)
                rows.append(
                    f'<tr class="{cls}"><td>{i}</td><td>{count}</td>'
                    f'<td class="src">{html.escape(src_line.rstrip())}</td></tr>'
                )
    except OSError:
        rows = [
            f'<tr><td colspan="3">Source not found: {html.escape(src_path)}</td></tr>'
        ]
    covered = sum(1 for h in line_hits.values() if h > 0)
    total = len(line_hits)
    pct = coverage_pct(covered, total)
    cls = pct_class(pct)
    return (
        f"<!DOCTYPE html><html><head><title>{html.escape(rel)}</title>"
        f"<style>{CSS}</style></head><body>"
        f'<p><a href="index.html">← index</a></p>'
        f"<h2>{html.escape(rel)}</h2>"
        f'<p class="{cls}">{covered}/{total} lines ({pct:.1f}%)</p>'
        f'<table><tr><th>#</th><th>hits</th><th>source</th></tr>'
        f"{''.join(rows)}</table></body></html>"
    )


def index_html(stats: dict[str, tuple[int, int]]) -> str:
    rows = []
    for rel, (covered, total) in sorted(stats.items()):
        pct = coverage_pct(covered, total)
        cls = pct_class(pct)
        slug = rel.replace("/", "__").replace(".", "_")
        rows.append(
            f'<tr><td><a href="{slug}.html">{html.escape(rel)}</a></td>'
            f"<td>{total}</td><td>{covered}</td>"
            f'<td class="{cls}">{pct:.1f}%</td></tr>'
        )
    total_l = sum(t for _, t in stats.values())
    total_c = sum(c for c, _ in stats.values())
    overall = coverage_pct(total_c, total_l)
    cls = pct_class(overall)
    return (
        "<!DOCTYPE html><html><head><title>Coverage</title>"
        f"<style>{CSS}</style></head><body>"
        "<h1>Coverage report</h1>"
        f'<p class="{cls}">Overall: {total_c}/{total_l} lines ({overall:.1f}%)</p>'
        '<table><tr><th>File</th><th>Lines</th><th>Covered</th><th>%</th></tr>'
        f"{''.join(rows)}</table></body></html>"
    )


def build_site(out: pathlib.Path) -> None:
    records = parse_lcov(REPORT)
    if not records:
        sys.exit(
            f"No coverage data at:\n  {REPORT}\n\nRun: bazel coverage //... first."
        )

    stats: dict[str, tuple[int, int]] = {}
    for sf, line_hits in records.items():
        if not any(sf.startswith(p) for p in _INCLUDE_PREFIXES):
            continue
        if not line_hits:
            continue
        covered = sum(1 for h in line_hits.values() if h > 0)
        total = len(line_hits)
        slug = sf.replace("/", "__").replace(".", "_")
        (out / f"{slug}.html").write_text(file_html(sf, line_hits))
        stats[sf] = (covered, total)

    if not stats:
        sys.exit("Coverage data found but no workspace source files matched.")

    (out / "index.html").write_text(index_html(stats))


def main() -> None:
    with tempfile.TemporaryDirectory() as tmpdir:
        out = pathlib.Path(tmpdir)
        print("Generating HTML report...")
        build_site(out)

        handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=tmpdir)
        print(f"\nServing at http://localhost:{PORT}  (Ctrl-C to stop)\n")
        with socketserver.TCPServer(("", PORT), handler) as srv:
            srv.allow_reuse_address = True
            try:
                srv.serve_forever()
            except KeyboardInterrupt:
                pass


if __name__ == "__main__":
    main()
