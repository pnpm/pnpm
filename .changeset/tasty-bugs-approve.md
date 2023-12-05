---
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolve-dependencies": patch
"@pnpm/modules-cleaner": patch
"@pnpm/headless": patch
"@pnpm/core": patch
"pnpm": patch
---

When `dedupe-direct-deps` is set to `true`, commands of dependencies should be deduplicated [#7359](https://github.com/pnpm/pnpm/pull/7359).
