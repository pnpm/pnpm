---
"@pnpm/config": major
"pnpm": major
---

By default don't run lifecycle scripts of dependencies during installation. In order to allow lifecycle scripts of specific dependencies, they should be listed in the `pnpm.onlyBuiltDependencies` field of `package.json` [#8897](https://github.com/pnpm/pnpm/pull/8897).
