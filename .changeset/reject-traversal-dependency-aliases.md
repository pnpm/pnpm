---
"@pnpm/resolve-dependencies": patch
"@pnpm/symlink-dependency": patch
"pnpm": patch
---

Reject dependency aliases that aren't a valid npm package name. A transitive registry package could previously use an alias like `@x/../../../../../.git/hooks` to make `pnpm install` create a symlink outside the intended `node_modules` directory, because the alias was passed straight into `path.join(modulesDir, alias)` without checking that the joined path stayed inside `modulesDir`. Aliases are now validated at manifest-read time (both the importer's manifest and every transitive package manifest) and re-checked at the symlink layer as defense in depth.
