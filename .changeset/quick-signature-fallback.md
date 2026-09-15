---
"@pnpm/deps.security.signatures": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update`, `pnpm with`, and automatic package-manager version switching no longer wait through registry retry delays when a configured registry has no signatures and `registry.npmjs.org` is unavailable [#14483](https://github.com/pnpm/pnpm/issues/14483).
