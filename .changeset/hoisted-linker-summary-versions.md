---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

The install summary now names the version each dependency resolved to when `node-linker` is `hoisted`. Dependencies restored after `node_modules` is deleted appear in the summary. Unsupported optional dependencies removed from `node_modules` also appear. Version changes show both the old and new versions [#15161](https://github.com/pnpm/pnpm/issues/15161).
