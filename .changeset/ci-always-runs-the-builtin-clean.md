---
"pacquet": patch
---

`pnpm ci` now empties `node_modules` before installing in a project that declares a `clean` script. It ran that script in place of the removal.
