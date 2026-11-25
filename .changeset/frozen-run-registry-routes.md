---
"pnpm": patch
---

Fixed repeated installs before `pnpm run` and `pnpm exec` after a scope registry changes when the command would run a frozen or filtered install [#18448](https://github.com/pnpm/pnpm/issues/18448).
