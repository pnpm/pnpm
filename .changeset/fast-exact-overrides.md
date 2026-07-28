---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Speed up installs after adding or changing an exact version override when the replacement package can reuse the dependency resolutions already recorded in the lockfile.
