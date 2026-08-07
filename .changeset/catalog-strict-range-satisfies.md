---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm update <pkg>@<version>` under `catalogMode: strict` no longer rejects a version that satisfies a range-based catalog entry, only a version that actually disagrees with it.
