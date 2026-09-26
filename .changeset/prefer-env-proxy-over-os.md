---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` uses `https_proxy`, `http_proxy`, and the proxy settings in `.npmrc` or `pnpm-workspace.yaml` ahead of the Windows or macOS system proxy. An empty `https_proxy` or `http_proxy` still means no proxy. When none of those settings is present, pnpm uses the operating system proxy [pnpm/pnpm#8561](https://github.com/pnpm/pnpm/issues/8561).
