---
"pacquet": patch
---

`pnpm store add` indexes tarball URLs without supplied integrity values using their computed integrity. It retries temporary store-index write contention and reports an error if a package cannot be indexed [pnpm/pnpm#18194](https://github.com/pnpm/pnpm/issues/18194).
