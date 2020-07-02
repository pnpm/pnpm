---
"@pnpm/global-bin-dir": minor
---

`globalBinDir()` may accept an array of suitable executable directories.
If one of these directories is in PATH and has bigger priority than the
npm/pnpm/nodejs directories, then that directory will be used.
