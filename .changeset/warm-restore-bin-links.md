---
"pacquet": patch
---

Sped up restoring a workspace with many projects from a warm global virtual store. Linking each project's direct dependencies no longer rereads every dependency's package.json, probes each bin script once instead of once per project, and writes fresh bin shims without probing for stale entries first [#14540](https://github.com/pnpm/pnpm/issues/14540).
