---
"@pnpm/core": minor
"pnpm": minor
---

`pnpm add` will now check if the added dependency is in the default workspace catalog. If the added dependency is present in the default catalog and the version requirement of the added dependency matches the default catalog's version, then the `pnpm add` will use the `catalog:` protocol. Note that if no version is specified in the `pnpm add` it will match the version described in the default catalog. If the added dependency does not match the default catalog's version it will use the default `pnpm add` behavior [#8640](https://github.com/pnpm/pnpm/issues/8640).
