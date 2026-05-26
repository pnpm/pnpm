---
"@pnpm/engine.runtime.commands": minor
"pnpm": minor
---

`pnpm runtime set <name> <version>` now saves the runtime to `devEngines.runtime` by default instead of `engines.runtime`. Pass `--save-prod` (or `-P`) to save it to `engines.runtime` instead [#11948](https://github.com/pnpm/pnpm/issues/11948).
