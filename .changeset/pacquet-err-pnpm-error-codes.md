---
"pacquet": patch
---

Every error code is now an `ERR_PNPM_*` code, matching the codes pnpm has always used. Errors previously reported internal Rust-crate codes such as `pacquet_package_manager::outdated_lockfile` or unprefixed codes such as `GIT_CHECKOUT_FAILED`; these are now `ERR_PNPM_OUTDATED_LOCKFILE` and `ERR_PNPM_GIT_CHECKOUT_FAILED`. Where pnpm defines a code for the same error, pnpm's exact code is used. Scripts and CI that match on the old codes need updating.
