---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

Sped up installs of projects whose local directory dependencies share transitive local directory dependencies. pnpm no longer checks a shared directory again for each direct dependency that reaches it [#17374](https://github.com/pnpm/pnpm/issues/17374).
