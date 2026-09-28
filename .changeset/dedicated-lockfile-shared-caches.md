---
"pacquet": patch
---

`pnpm install` in a workspace with `sharedWorkspaceLockfile: false` uses less CPU and memory when several projects depend on the same packages. The installs of the projects now share their package metadata, lockfile verification, and store caches [#14480](https://github.com/pnpm/pnpm/issues/14480).
