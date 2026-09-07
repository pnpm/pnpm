---
"pacquet": patch
---

`pnpm install` no longer fails with `Invalid CA certificate` when a `ca` or `cafile` setting carries something pnpm cannot read as a certificate. The unreadable entry is ignored, the way pnpm 11 ignores it [#14646](https://github.com/pnpm/pnpm/issues/14646). A `cert` or `key` setting with a blank value now reads as unset.
