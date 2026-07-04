---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm no longer warns about ignored project-level auth settings when `PNPM_CONFIG_NPMRC_AUTH_FILE` points at the project `.npmrc` — setting it to that file is an explicit opt-in to trusting it, so auth env variables in it are expanded [pnpm/pnpm#12480](https://github.com/pnpm/pnpm/issues/12480).
