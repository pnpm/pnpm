---
"pnpm": patch
"pacquet": patch
---

`pnpm clean` now removes a `node_modules` directory that is left empty after cleaning. A `node_modules` that still holds a preserved dotfile, such as `.cache`, is kept [#13390](https://github.com/pnpm/pnpm/issues/13390).
