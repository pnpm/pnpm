---
"pacquet": patch
---

Sped up installs that have no lockfile. pnpm now links packages whose dependency subtree has no peer dependencies into the virtual store while resolution is still running.
