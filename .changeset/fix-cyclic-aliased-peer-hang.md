---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fix pnpm hanging during peer resolution when an aliased install pulls in transitive packages with mutual peer cycles at different depths in the dependency tree (for example, `pnpm i nuxt@npm:nuxt-nightly@5x`). Cycles whose members hit the `findHit` cache instead of running their own `calculateDepPath` are now short-circuited by sibling resolutions at the level where the cycle is detected, so the cached path promises no longer deadlock. [#11999](https://github.com/pnpm/pnpm/issues/11999).
