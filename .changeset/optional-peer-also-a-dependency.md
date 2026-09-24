---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

pnpm now installs a dependency that a package also declares as an optional peer dependency, for example `lightningcss` in some vite builds. The dependency was missing from `node_modules`, so the package failed to import it [#8912](https://github.com/pnpm/pnpm/issues/8912).
