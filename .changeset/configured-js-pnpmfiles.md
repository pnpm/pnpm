---
"pacquet": patch
---

pnpm loads a configured `.js` pnpmfile as CommonJS or as an ES module, following the nearest `package.json`. The install used to fail with `ERR_PNPM_PNPMFILE_NOT_FOUND` for every `.js` pnpmfile [pnpm/pnpm#15141](https://github.com/pnpm/pnpm/issues/15141). An ES module pnpmfile may also export its hooks with `export default`.
