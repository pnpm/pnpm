---
"pacquet": patch
---

Empty `http-proxy`, `https-proxy`, or `proxy` values — from the `.npmrc`, the CLI, or the `HTTP_PROXY` / `HTTPS_PROXY` / `PROXY` environment variables — are now treated as "no proxy", matching the TypeScript pnpm CLI. Previously pnpm failed with `ERR_PNPM_INVALID_PROXY` when one of these was set to an empty string, e.g. a shell exporting `HTTP_PROXY=` to disable a proxy [#13533](https://github.com/pnpm/pnpm/issues/13533).
