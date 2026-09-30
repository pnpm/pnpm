---
"@pnpm/pnpr": patch
---

pnpr now computes a missing or empty `dist.integrity` for upstream package versions, so pnpm clients can install them. It pins up to 64 versions each time it fetches a packument from an upstream with caching enabled [pnpm/tasks#24](https://github.com/pnpm/tasks/issues/24).
