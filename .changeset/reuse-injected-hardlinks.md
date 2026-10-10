---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

Sped up frozen installs of unchanged injected workspace packages that use hardlinks.
