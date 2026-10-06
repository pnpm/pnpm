---
"@pnpm/fetching.directory-fetcher": patch
"pnpm": patch
---

`pnpm deploy` with `deployAllFiles` now rejects symlinks that point outside the package directory. Local package installs with this setting apply the same check.
