---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Adding a package to a workspace no longer forces a full re-resolution when every dependency it declares is already locked for a sibling. The lockfile update writes the new project's importer entry from the versions the lockfile already holds; a dependency no locked version satisfies still reaches the resolver [#13696](https://github.com/pnpm/pnpm/issues/13696).
