---
"@pnpm/lockfile.to-pnp": patch
"pnpm": patch
"pacquet": patch
---

Fixed package maps resolving workspace and linked packages to the wrong directory when a path contains `#`, `?`, or `%`.
