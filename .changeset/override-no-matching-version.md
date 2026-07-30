---
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

When an `overrides` entry points at a version that does not exist on the registry, `pnpm install` now reports the override entry itself (and the latest version matching its selector) instead of attributing the failure to whichever package happened to depend on it.
