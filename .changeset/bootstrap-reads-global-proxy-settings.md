---
"@pnpm/config.reader": patch
"pnpm": patch
---

The proxy settings `httpProxy`, `httpsProxy`, `noProxy`, `proxy`, and `noproxy` set in the global `config.yaml`, through `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, or as `--config.httpsProxy` on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. Previously, these requests read the proxy only from `.npmrc` and the `HTTPS_PROXY` family of variables [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).
