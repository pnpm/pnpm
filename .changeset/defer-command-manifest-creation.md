---
"pacquet": patch
---

Commands that do not write the project manifest now leave a missing `package.json` absent. This covers queries such as `pnpm why` and `pnpm outdated`, `pnpm fetch`, `pnpm dedupe --check`, and commands that exit with an error, such as `pnpm rt ls`. These commands used to create one from the initialization template. `pnpm sbom` run from a lockfile alone now reports the root component as `unknown` with version `0.0.0` [pnpm/pnpm#16907](https://github.com/pnpm/pnpm/issues/16907).
