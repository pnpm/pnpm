---
"pacquet": patch
---

`pnpm store add` retries temporary store-index write contention and exits with an error if a package cannot be indexed [pnpm/pnpm#18194](https://github.com/pnpm/pnpm/issues/18194).
