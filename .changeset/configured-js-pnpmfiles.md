---
"pacquet": patch
---

A configured `.js` pnpmfile no longer fails the install with `ERR_PNPM_PNPMFILE_NOT_FOUND`. pnpm loads it as CommonJS or as an ES module, following the nearest `package.json` [pnpm/pnpm#15141](https://github.com/pnpm/pnpm/issues/15141). An ES module pnpmfile may also export its hooks with `export default`.
