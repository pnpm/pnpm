---
"pacquet": patch
---

Sped up installs in large workspaces. The check that decides whether the lockfile needs updating no longer compares every project against every lockfile entry [#14352](https://github.com/pnpm/pnpm/issues/14352).
