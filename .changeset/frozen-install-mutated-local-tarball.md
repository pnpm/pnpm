---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now checks the integrity of local tarball dependencies and fails when a local archive has changed on disk.
