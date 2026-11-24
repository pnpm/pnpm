---
"pnpm": minor
---

When an install finishes, its progress lines are replaced by the stats, such as `Packages: +58 -2 (90% downloaded)`, with the share of packages downloaded or reused from the store. In the bar below it, a downloaded package is drawn as `↓` and a reused one as `+`.

The `Scope:` line is printed only when a filter selected some of the workspace projects, not when the command ran on all of them.
