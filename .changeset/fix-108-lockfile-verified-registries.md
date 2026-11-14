---
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.utils": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now re-verifies lockfile resolutions when the default registry or a scoped registry changes [#18070](https://github.com/pnpm/pnpm/pull/18070).
