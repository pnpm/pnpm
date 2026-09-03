---
"pacquet": patch
---

Sped up installs that restore a deleted `node_modules` from a warm global virtual store. pnpm now trusts an existing package directory in the global virtual store and only recreates the project links, so a restore no longer re-links every package in the store [#14510](https://github.com/pnpm/pnpm/issues/14510).
