---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

Sped up installs of projects whose local directory dependencies share transitive local directory dependencies. pnpm now checks each shared directory once per project [#17374](https://github.com/pnpm/pnpm/issues/17374).
