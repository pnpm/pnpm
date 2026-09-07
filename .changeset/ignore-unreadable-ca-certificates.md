---
"pacquet": patch
---

`pnpm install` no longer fails with `Invalid CA certificate` when a `ca` or `cafile` setting carries something pnpm cannot read as a certificate. The unreadable certificate is ignored and the readable ones around it still apply. A `cert` or `key` setting with a blank value now reads as unset [#14646](https://github.com/pnpm/pnpm/issues/14646).
