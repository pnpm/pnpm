---
"@pnpm/headless": patch
"pnpm": patch
---

The upload of built artifacts (side effects) should not fail when `node-linker` is set to `hoisted` and installation runs on a project that already had a `node_modules` directory [#5823](https://github.com/pnpm/pnpm/issues/5823).

This fixes a bug introduced by [#5814](https://github.com/pnpm/pnpm/pull/5814).
