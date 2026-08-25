---
"@pnpm/deps.graph-hasher": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

With `virtualStoreType: global`, a dependency's lifecycle scripts run inside the store, where the directory above their `node_modules` is a store slot rather than the project that installed them. A package that reads the manifest there — the convention git-hook installers use to find their consumer — took the install down with an `ENOENT`. Every slot now carries a manifest that declares nothing, so the read resolves and the install proceeds [#13318](https://github.com/pnpm/pnpm/issues/13318).

pnpm does not run a dependency's lifecycle scripts on behalf of the project that installed it, and a shared store makes that plain: one build serves every project that resolves the same dependencies. A package that has to act on your repository — installing git hooks, for example — belongs in a script of your own project, which does run there:

```json
{
  "scripts": {
    "prepare": "simple-git-hooks"
  }
}
```
