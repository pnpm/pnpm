---
"@pnpm/exec.lifecycle": patch
"pnpm": patch
---

User-defined `npm_config_*` environment variables are now preserved during lifecycle script execution. Previously, all `npm_`-prefixed env vars were stripped, which caused user-set variables like `npm_config_platform_arch` to be lost [pnpm/pnpm#12399](https://github.com/pnpm/pnpm/issues/12399).
