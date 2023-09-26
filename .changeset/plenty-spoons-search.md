---
"@pnpm/headless": patch
"@pnpm/core": patch
"pnpm": patch
---

When the `node-linker` is set to `hoisted`, the `package.json` files of the existing dependencies inside `node_modules` will be checked to verify their actual versions. The data in the `node_modules/.modules.yaml` and `node_modules/.pnpm/lock.yaml` may not be fully reliable, as an installation may fail after changes to dependencies were made but before those state files were updated [#7107](https://github.com/pnpm/pnpm/pull/7107).

