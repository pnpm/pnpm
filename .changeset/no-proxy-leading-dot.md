---
"pacquet": patch
---

Fixed `NO_PROXY` entries that start with a dot, such as `.npmjs.org`, so they bypass the proxy for the domain and its subdomains [#14686](https://github.com/pnpm/pnpm/issues/14686).
