---
"pacquet": patch
---

`pnpm install` now re-resolves a dependency when its manifest range is updated from a prerelease to a stable version. The lockfile previously retained the prerelease version and caused `--frozen-lockfile` to fail [pnpm/pnpm#15528](https://github.com/pnpm/pnpm/issues/15528).
