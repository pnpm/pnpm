---
"@pnpm/network.auth-header": patch
"pnpm": patch
---

Authorization token should be found in the configuration, when the requested URL is explicitly specified with a default port (443 on HTTPS or 80 on HTTP) [#6863](https://github.com/pnpm/pnpm/pull/6864).
