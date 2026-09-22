---
"pacquet": patch
---

`pnpm ci` now empties `node_modules` before installing in a project that declares a `clean` script [#15276](https://github.com/pnpm/pnpm/issues/15276). It ran that script in place of the removal.
