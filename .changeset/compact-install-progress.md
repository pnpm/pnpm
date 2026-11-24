---
"pnpm": minor
---

The progress of an install is shown on one line. While installing, it counts the packages as they are downloaded or reused, such as `Packages: 90 downloaded, 5 reused`, and then as they are added, such as `Packages: +12 of 95`. A bar below it gets a sign for each package, yellow once the package is downloaded or reused and green once it is added. In an install from a lockfile, each sign appears in the default color as soon as the package is known. When the install finishes, the same line shows the result, such as `Packages: +58 -2 (90% downloaded)`, with the share of packages downloaded or reused from the store. In the bar, a downloaded package is drawn as `↓` and a reused one as `+`. In CI and other output that is not a terminal, each update is printed as its own line, without the bar.

The `Scope:` line is printed only when a filter selected some of the workspace projects, not when the command ran on all of them.

The line of a large download is cleared when it finishes, and output that is not a terminal no longer prints download lines.
