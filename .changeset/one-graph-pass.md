---
"pacquet": patch
---

Sped up installs in large workspaces: the workspace dependency graph is now built and searched for cycles once per run instead of twice, and its edges resolve in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).
