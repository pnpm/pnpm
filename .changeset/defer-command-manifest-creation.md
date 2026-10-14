---
"pacquet": patch
---

Commands that do not write the project manifest no longer create a missing `package.json`. This covers queries such as `pnpm why` and `pnpm outdated`, `pnpm fetch`, `pnpm dedupe --check`, and commands that exit with an error, such as `pnpm rt ls` or `pnpm audit` without a lockfile. `pnpm sbom` run from a lockfile alone no longer reports the root component with the name, version, and license of the initialization template [pnpm/pnpm#16907](https://github.com/pnpm/pnpm/issues/16907).
