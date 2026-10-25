#!/usr/bin/env python3
"""Compare two self-test output folders picture by picture, exact pixels.

    python3 tools/selftest_diff.py <before> <after> [--noise <dir> ...]

<before> and <after> are `poni.exe --selftest` folders (tools/run-selftest-wine.sh),
usually the same theme and DPI from two builds. Every PNG in <before> is
compared with the one of the same name in <after>, pixel by pixel with no
tolerance. The script lists each picture that differs, how many pixels changed
and the box around them, and the pictures only one side has. A scene appended
to the self-test shows as "only in after"; a picture missing from <after>
means a scene moved or was lost.

Some pixels change between two runs of the same build: the clock shows the
minute the picture was taken (run both builds side by side, as
tools/site_shots.py says), and the equalizer, a blink and Oyen move with time.
Give more runs of the <before> build as --noise (as many as you have): a
picture that differs between <before> and any of them is listed as
time-dependent and doesn't count.

Exit code: 0 when no picture differs (beyond --noise) and nothing in <before>
is missing from <after>, 1 otherwise, 2 for a usage problem.
"""

import argparse
import json
import signal
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def pictures(d):
    return {p.name: p for p in sorted(d.glob("*.png"))}


def load(path):
    with Image.open(path) as im:
        if im.mode not in ("L", "RGB", "RGBA"):
            im = im.convert("RGBA")
        return np.asarray(im)


def compare(a_path, b_path):
    """None when identical, else (changed pixels, total pixels, box or size note)."""
    a, b = load(a_path), load(b_path)
    if a.shape != b.shape:
        return (None, None, f"size {a.shape[1]}x{a.shape[0]} -> {b.shape[1]}x{b.shape[0]}")
    diff = a != b
    if diff.ndim == 3:
        diff = diff.any(axis=2)
    n = int(diff.sum())
    if n == 0:
        return None
    ys, xs = np.nonzero(diff)
    box = f"x {xs.min()}-{xs.max()}, y {ys.min()}-{ys.max()}"
    return (n, diff.size, box)


def failed_checks(d):
    try:
        return set(json.loads((d / "report.json").read_text(encoding="utf-8")).get("failed_checks", []))
    except (OSError, ValueError):
        return None


def main():
    ap = argparse.ArgumentParser(description="Compare two self-test output folders, exact pixels.")
    ap.add_argument("before", type=Path)
    ap.add_argument("after", type=Path)
    ap.add_argument("--noise", type=Path, action="append", default=[], help="another run of the <before> build (repeatable): pictures that differ there too are time-dependent")
    args = ap.parse_args()
    for d in [args.before, args.after] + args.noise:
        if not d.is_dir():
            print(f"not a folder: {d}", file=sys.stderr)
            return 2

    before, after = pictures(args.before), pictures(args.after)
    noise = [pictures(d) for d in args.noise]
    print(f"before: {args.before} ({len(before)} pictures)")
    print(f"after:  {args.after} ({len(after)} pictures)")
    for d, p in zip(args.noise, noise):
        print(f"noise:  {d} ({len(p)} pictures)")

    same, changed, timed = [], [], []
    for name, a in before.items():
        if name not in after:
            continue
        d = compare(a, after[name])
        if d is None:
            same.append(name)
        elif any(name in p and compare(a, p[name]) is not None for p in noise):
            timed.append((name, d))
        else:
            changed.append((name, d))

    def line(name, d):
        n, total, note = d
        if n is None:
            return f"  {name:44s} {note}"
        return f"  {name:44s} {n:8d} px ({100.0 * n / total:6.3f} %)  in {note}"

    print(f"identical: {len(same)}")
    if changed:
        print(f"differ: {len(changed)}")
        for name, d in changed:
            print(line(name, d))
    if timed:
        print(f"time-dependent (they differ between two runs of the same build too): {len(timed)}")
        for name, d in timed:
            print(line(name, d))
    gone = [n for n in before if n not in after]
    new = [n for n in after if n not in before]
    if gone:
        print(f"only in before ({len(gone)}): {', '.join(gone)}")
    if new:
        print(f"only in after ({len(new)}): {', '.join(new)}")

    fb, fa = failed_checks(args.before), failed_checks(args.after)
    if fb is not None and fa is not None:
        print(f"report.json failed checks: before {len(fb)}, after {len(fa)}" + (f"; new in after: {', '.join(sorted(fa - fb))}" if fa - fb else ""))

    ok = not changed and not gone
    print("result: " + ("no picture changed" if ok else "pictures changed"))
    return 0 if ok else 1


if __name__ == "__main__":
    # Quiet when piped into head.
    if hasattr(signal, "SIGPIPE"):
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    sys.exit(main())
