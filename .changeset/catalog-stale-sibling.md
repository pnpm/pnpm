---
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

A project that wasn't part of an install that moved a catalog entry now follows the entry the next time it is installed. It used to keep the version the entry resolved to before — a version the entry no longer allowed — and no later install corrected it, so one catalog entry ended up resolved to two versions.
