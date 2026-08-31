---
"pacquet": patch
---

Fixed non-ASCII characters in configuration values being mangled during environment-variable substitution. Paths such as `storeDir: ./café-store` are now preserved [#14383](https://github.com/pnpm/pnpm/issues/14383).
