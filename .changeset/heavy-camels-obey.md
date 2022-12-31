---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-publishing": patch
"@pnpm/filter-workspace-packages": patch
"@pnpm/get-context": patch
"@pnpm/core": patch
"pnpm": patch
---

replace dependency `is-ci` by `ci-info` (`is-ci` is just a simple wrapper around `ci-info`).
