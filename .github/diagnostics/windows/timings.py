"""Summarize a cargo --timings report: where a test build spends its time.

    timings.py <cargo-timing.html> [label]
"""
import collections
import json
import re
import subprocess
import sys


def workspace_packages():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True, text=True, check=True,
    ).stdout)
    return {package["name"] for package in meta["packages"]}


def main(path, label):
    html = open(path, encoding="utf-8").read()
    units = json.loads(re.search(r"const UNIT_DATA = (\[.*?\]);", html, re.S).group(1))
    ours = workspace_packages()
    wall = max(unit["start"] + unit["duration"] for unit in units)
    groups = collections.defaultdict(lambda: [0.0, 0])
    for unit in units:
        target = unit.get("target", "")
        if unit["name"] not in ours:
            kind = "third-party"
        elif "test" in target or "bench" in target:
            kind = "workspace test harness"
        elif "build-script" in target or unit.get("mode") == "run-custom-build":
            kind = "workspace build script"
        else:
            kind = "workspace lib/bin"
        groups[kind][0] += unit["duration"]
        groups[kind][1] += 1
    print(f"== {label}: {len(units)} units, wall {wall:.0f} s")
    for kind, (seconds, count) in sorted(groups.items(), key=lambda item: -item[1][0]):
        print(f"  {kind:26} {count:4} units  {seconds:7.0f} s summed")
    harness_end = max((u["start"] + u["duration"] for u in units if u["name"] in ours), default=0)
    third_end = max((u["start"] + u["duration"] for u in units if u["name"] not in ours), default=0)
    print(f"  last third-party unit ends at {third_end:.0f} s, last workspace unit at {harness_end:.0f} s")
    print("  slowest units:")
    for unit in sorted(units, key=lambda u: -u["duration"])[:15]:
        print(f"    {unit['duration']:6.1f} s  {unit['name']}{unit.get('target', '')}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else sys.argv[1])
