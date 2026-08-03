---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

An empty `http-proxy`, `https-proxy`, `proxy`, or `no-proxy` value — from the `.npmrc`, `pnpm-workspace.yaml`, the CLI, or the `HTTP_PROXY` / `HTTPS_PROXY` / `PROXY` / `NO_PROXY` environment variables — no longer fails the install with `ERR_PNPM_INVALID_PROXY`. Empty settings are treated as unset, so a shell exporting `HTTP_PROXY=` disables the proxy, and an empty setting in a config file or on the command line lets the next source in the cascade apply — an empty `proxy=` in the `.npmrc` no longer suppresses `HTTPS_PROXY` [#13533](https://github.com/pnpm/pnpm/issues/13533).

`proxy=false` in the `.npmrc` or `proxy: false` in `pnpm-workspace.yaml` now turns proxying off instead of being read as a proxy host named `false`. As before, `false` and `null` on `https-proxy` / `http-proxy` / `no-proxy` mean "not configured", so the next source in the cascade applies.
