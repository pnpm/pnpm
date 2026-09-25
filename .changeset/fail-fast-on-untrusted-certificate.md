---
"@pnpm/network.fetch": patch
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
---

`pnpm install` now fails at once when a registry or tarball server presents a TLS certificate that fails verification, such as a self-signed or expired one. The error names the certificate problem. Such requests were retried for more than a minute [#9134](https://github.com/pnpm/pnpm/issues/9134).
