---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/releasing.commands": patch
"@pnpm/lockfile.fs": patch
"@pnpm/lockfile.utils": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm deploy --offline` requiring registry metadata for dependencies already pinned in the source lockfile and available in the store. This covers per-project lockfiles, `--legacy`, and npm aliases. `--trust-lockfile` now also skips supply-chain policy checks when resolving unchanged locked dependencies [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).
