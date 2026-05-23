---
"@pnpm/installing.env-installer": patch
"pnpm": patch
---

Fixed `pnpm add --config` leaving orphan entries in `pnpm-lock.env.yaml` (the optional subdependencies of the previously resolved version of the updated config dependency).
