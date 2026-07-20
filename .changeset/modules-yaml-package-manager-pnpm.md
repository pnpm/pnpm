---
"pacquet": patch
---

`node_modules/.modules.yaml` now records `packageManager` as `pnpm@<release version>` (for example `pnpm@12.0.0-alpha.13`), matching `pnpm --version` and the TypeScript CLI. It previously recorded the internal crate name and crate version, `pacquet@0.0.1`.
