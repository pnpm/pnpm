---
"@pnpm/network.fetch": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm publish` now honors the configured HTTP/HTTPS proxy (including `https_proxy`/`http_proxy`/`no_proxy` environment variables) when polling the registry's `doneUrl` during the web-based authentication flow. Previously the poll bypassed the proxy, causing the registry to respond `403` from a different source IP and the login to never complete [#11561](https://github.com/pnpm/pnpm/issues/11561).
