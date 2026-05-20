---
"@pnpm/config.pick-registry-for-package": patch
"pnpm": patch
---

Fix `pickRegistryForPackage` returning the wrong registry for an unscoped `npm:` alias under a scoped local name. A manifest entry like `"@private/foo": "npm:lodash@^1"` was routing the `lodash` fetch through `registries["@private"]`, even though `lodash` is unscoped and doesn't live on that registry. The npm-alias branch now returns the alias target's own scope (or `null` for an unscoped target, falling through to `registries.default`) instead of leaking into the local key's scope.
