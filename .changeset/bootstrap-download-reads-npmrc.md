---
"pacquet": patch
---

When the `pnpm` package has to download its native binary on first run, it now uses the registry and credentials from `.npmrc` and from the `npm_config_registry` and `pnpm_config_registry` environment variables. `COREPACK_NPM_REGISTRY` still takes precedence. A project `.npmrc` is not read when `COREPACK_INTEGRITY_KEYS` turns off the signature check [#16655](https://github.com/pnpm/pnpm/issues/16655).
