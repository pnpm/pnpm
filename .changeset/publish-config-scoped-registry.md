---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm publish` now honors `publishConfig["@scope:registry"]` for a package in that scope. It takes precedence over the registry set for the same scope in `.npmrc` and over `publishConfig.registry` [#12071](https://github.com/pnpm/pnpm/issues/12071).
