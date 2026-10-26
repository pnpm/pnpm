---
"@pnpm/config.reader": patch
"@pnpm/types": patch
"pnpm": patch
"pacquet": patch
---

`strictSsl` and `localAddress` set in the global `config.yaml`, through `PNPM_CONFIG_STRICT_SSL` and `PNPM_CONFIG_LOCAL_ADDRESS`, or on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. These requests read them from `.npmrc` only before. `--config.localAddress` now reaches them in the camelCase spelling too [pnpm/tasks#201](https://github.com/pnpm/tasks/issues/201).
