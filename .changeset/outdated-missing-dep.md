---
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm outdated` and `pnpm -r outdated` now fail with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when a requested package selector does not match any dependency in the inspected projects [#2319](https://github.com/pnpm/pnpm/issues/2319).
