---
"pacquet": patch
"@pnpm/pnpr": patch
---

Requests to a registry or tarball server whose TLS certificate fails verification now fail at once. Such requests were retried for more than a minute without any output [#9134](https://github.com/pnpm/pnpm/issues/9134).
