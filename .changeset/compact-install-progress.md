---
"pnpm": minor
---

The progress output of an install is shorter. In a terminal, the live progress line is cleared when the install finishes, and one line such as `Packages: +5 -2 (60% downloaded)` remains. It shows the larger share of packages downloaded or reused from the store. The `+++--` bar is gone. In CI and other output that is not a terminal, the final progress counts are still printed before that line.

On a first install, the import method and the store paths are printed on one line, such as `Packages are hard linked from the store at ~/.local/share/pnpm/store/v11 into node_modules/.pnpm`.

The `Scope:` line is printed only when a filter selected some of the workspace projects, not when the command ran on all of them.

The line of a large download is cleared when it finishes, and output that is not a terminal no longer prints download lines.
