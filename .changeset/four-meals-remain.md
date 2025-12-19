---
"@pnpm/git-fetcher": major
---

`pnpm` now correctly handles git dependencies pinned to an annotated tag by resolving them to the underlying commit before validation.

This prevents false `GIT_CHECKOUT_FAILED` errors while preserving strict checks for partial or invalid commit hashes.
