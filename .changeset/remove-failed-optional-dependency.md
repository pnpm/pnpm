---
"@pnpm/building.during-install": patch
"@pnpm/building.after-install": patch
"pacquet": patch
"pnpm": patch
---

pnpm now removes an optional dependency from `node_modules` if its install script fails, unless the global virtual store is enabled. Code that checks whether the package is installed no longer finds a package that cannot load [#8756](https://github.com/pnpm/pnpm/issues/8756).
