---
"pnpm": minor
---

The progress of an install is shown on one line. While installing, it reads `Packages: +12 of 58 resolved`, with a bar below it that grows as packages are added. When the install finishes, the same line shows the result, such as `Packages: +58 -2 (90% downloaded)`, with the share of packages downloaded or reused from the store. In the bar, a downloaded package is drawn as `↓` and a reused one as `+`. In CI and other output that is not a terminal, each update is printed as its own line, without the bar.

The `Scope:` line is printed only when a filter selected some of the workspace projects, not when the command ran on all of them.

The line of a large download is cleared when it finishes, and output that is not a terminal no longer prints download lines.
