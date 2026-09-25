---
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

When an overrides entry requests a version that does not exist on the registry, `pnpm install` now attributes the resolution error to the override entry and reports the latest version matching its selector.
