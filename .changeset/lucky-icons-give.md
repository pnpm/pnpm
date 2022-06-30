---
"@pnpm/core": patch
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Never skip lockfile resolution when the lockfile is not up-to-date and `--lockfile-only` is used. Even if `frozen-lockfile` is `true` [#4951](https://github.com/pnpm/pnpm/issues/4951).
