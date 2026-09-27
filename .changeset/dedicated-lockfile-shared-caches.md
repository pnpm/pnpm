---
"pacquet": patch
---

In a workspace with `sharedWorkspaceLockfile: false`, the installs of the projects now share their package metadata, lockfile verification, and store caches. Metadata that several projects depend on is read, parsed, and verified once per install instead of once per project [#14480](https://github.com/pnpm/pnpm/issues/14480).
