---
"pacquet": patch
---

`pnpm install` failed with `Invalid CA certificate` when a `ca` or `cafile` setting carried something pnpm could not read as a certificate. The unreadable certificate is now ignored and the readable ones around it still apply. A `cert` or `key` setting with a blank value now reads as unset [#14646](https://github.com/pnpm/pnpm/issues/14646).
