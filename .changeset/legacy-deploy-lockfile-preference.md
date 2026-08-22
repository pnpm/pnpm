---
"pacquet": patch
---

`pnpm deploy --legacy` now prefers dependency versions pinned in the source workspace lockfile when they still satisfy the deployed project's range [pnpm/pnpm#13857](https://github.com/pnpm/pnpm/issues/13857).
