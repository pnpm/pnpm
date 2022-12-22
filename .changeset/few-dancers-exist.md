---
"@pnpm/plugin-commands-rebuild": patch
"pnpm": patch
---

`pnpm rebuild` should not fail if node_modules was created by pnpm version 7.18 or older [#5815](https://github.com/pnpm/pnpm/issues/5815).
