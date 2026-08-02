---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

An empty `http-proxy`, `https-proxy`, `proxy`, or `no-proxy` value — from the `.npmrc`, `pnpm-workspace.yaml`, the CLI, or the `HTTP_PROXY` / `HTTPS_PROXY` / `PROXY` / `NO_PROXY` environment variables — no longer fails the install with `ERR_PNPM_INVALID_PROXY`. Empty settings are treated as unset, so a shell exporting `HTTP_PROXY=` disables the proxy, and an empty setting in a config file or on the command line lets the next source in the cascade apply — an empty `proxy=` in the `.npmrc` no longer suppresses `HTTPS_PROXY` [#13533](https://github.com/pnpm/pnpm/issues/13533).
