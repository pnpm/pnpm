---
"@pnpm/config.reader": patch
"pnpm": patch
---

Warnings about ignored environment variables in project `.npmrc` credentials now explain how to configure trusted credentials or explicitly trust the project auth file with `PNPM_CONFIG_NPMRC_AUTH_FILE` [pnpm/pnpm#15051](https://github.com/pnpm/pnpm/issues/15051).
