---
"@pnpm/store-connection-manager": minor
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Fix a bug in which pnpm downloads packages whose `libc` differ from `pnpm.supportedArchitectures.libc` [#7362](https://github.com/pnpm/pnpm/issues/7362).
