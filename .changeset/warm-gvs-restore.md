---
"pacquet": patch
---

Sped up installs that restore a deleted `node_modules` from a warm global virtual store. pnpm no longer re-links packages that are already fully present in the global virtual store [#14510](https://github.com/pnpm/pnpm/issues/14510).
