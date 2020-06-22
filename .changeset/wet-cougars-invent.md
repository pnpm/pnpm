---
"@pnpm/global-bin-dir": patch
---

When looking for suitable directories for global executables, ignore case.

When comparing to the currently running Node.js executable directory,
ignore any trailing slash. `/foo/bar` is the same as `/foo/bar/`.
