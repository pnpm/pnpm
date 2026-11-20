---
"@pnpm/deps.graph-hasher": patch
"@pnpm/deps.graph-builder": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/building.after-install": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.
