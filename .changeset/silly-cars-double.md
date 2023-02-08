---
"@pnpm/resolve-dependencies": patch
"@pnpm/resolver-base": patch
"@pnpm/npm-resolver": patch
"@pnpm/core": patch
"pnpm": patch
---

When resolving dependencies, prefer versions that are already used in the root of the project. This is important to minimize the number of packages that will be nested during hoisting [#6054](https://github.com/pnpm/pnpm/pull/6054).
