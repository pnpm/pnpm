---
"@pnpm/npm-resolver": patch
---

An internal refactor was performed to remove a misleading usage of `pMemoize`. Previously the `maxAge` argument was passed, but this field is ignored by the `p-memoize` NPM package.
