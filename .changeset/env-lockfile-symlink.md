---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

Reject symlinked `pnpm-lock.yaml` files when reading or writing the env lockfile document.
