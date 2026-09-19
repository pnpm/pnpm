---
"pacquet": patch
---

`pnpm install` and other commands that report configuration warnings now warn when environment variables in project `.npmrc` credentials are ignored. The warning explains how to configure credentials with `pnpm config set` [pnpm/pnpm#15051](https://github.com/pnpm/pnpm/issues/15051).
