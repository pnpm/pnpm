---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

pnpm v12 now shows warnings during installs and other commands that report config warnings when it ignores environment variables in project `.npmrc` credentials. The warning in pnpm v11 and v12 now explains how to configure trusted credentials or explicitly trust the project auth file with `PNPM_CONFIG_NPMRC_AUTH_FILE` [pnpm/pnpm#15051](https://github.com/pnpm/pnpm/issues/15051).
