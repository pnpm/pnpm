---
"@pnpm/bins.linker": patch
"pnpm": minor
---

Added the `pnpm bugs` command that opens the package's bug tracker URL in the browser.

Fix the `bins.linker` tests to use `process.execPath` for `spawnSync` to avoid empty stdout.
