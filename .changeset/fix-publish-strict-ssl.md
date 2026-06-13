---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed `pnpm publish` ignoring `strictSsl: false` when publishing to registries with self-signed certificates. The `strictSSL` option is now forwarded to `libnpmpublish` / `npm-registry-fetch` so that `strict-ssl=false` in `.npmrc` or `strictSsl: false` in `pnpm-workspace.yaml` is respected during publish, the same way it is for `pnpm install` [pnpm/pnpm#12012](https://github.com/pnpm/pnpm/issues/12012).
