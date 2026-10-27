---
"pacquet": patch
---

`pnpm install` now records the on-disk `package.json` manifests in the workspace state, so `pnpm run` and repeat installs do not report `node_modules` as out of date when `readPackage` hooks or `packageExtensions` modify a workspace project manifest [#17396](https://github.com/pnpm/pnpm/issues/17396).
