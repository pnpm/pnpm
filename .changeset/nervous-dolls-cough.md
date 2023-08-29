---
"@pnpm/plugin-commands-rebuild": major
"@pnpm/build-modules": major
"pnpm": patch
---

Default `skipIfHasSideEffectsCache` to `true`, fixing side-effect cache issue when `shared-workspace-lockfile` is `false` [#6890](https://github.com/pnpm/pnpm/issues/6890).
