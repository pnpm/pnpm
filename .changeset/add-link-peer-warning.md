---
"@pnpm/installing.commands": patch
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add <dir>` now warns when the added directory declares peer dependencies, as `pnpm link` does. The directory is saved as a `link:` dependency, and its peers are not resolved from the project that adds it. Use the `file:` protocol to have them resolved [#5523](https://github.com/pnpm/pnpm/issues/5523).
