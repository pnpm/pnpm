---
"@pnpm/lockfile.peer-edges": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --prod` no longer drops a dependency that a package declares both as an optional peer and as its own dependency, when the package resolved it as its own dependency and a dev dependency provides the same package under another alias.
