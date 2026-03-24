---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Parallelize hoist, symlinkDirectDependencies, and direct dep bin linking in headless install. These independent I/O operations now run concurrently via `Promise.all` instead of sequentially.
