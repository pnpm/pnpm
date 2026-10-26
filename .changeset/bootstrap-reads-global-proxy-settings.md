---
"@pnpm/config.reader": patch
"pnpm": patch
---

`httpProxy`, `httpsProxy`, and `noProxy` set in the global `config.yaml`, through `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, or as `--config.httpsProxy` on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. These requests read them from `.npmrc` and the `HTTPS_PROXY` family of variables only before [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).
