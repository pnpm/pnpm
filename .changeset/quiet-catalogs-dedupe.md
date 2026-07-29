---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `pnpm dedupe` updating valid catalog resolutions when another matching version exists in the lockfile.
