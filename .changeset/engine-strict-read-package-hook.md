---
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.package-requester": patch
"@pnpm/store.controller-types": patch
"pnpm": patch
---

`pnpm install --engine-strict` now respects `engines` relaxed by `readPackage` hooks in `.pnpmfile.cjs` [pnpm/pnpm#15482](https://github.com/pnpm/pnpm/issues/15482).
