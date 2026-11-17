---
"pacquet": minor
---

Added `pnpm why --ranges`, which lists the version range that each direct dependent declares for the package. `pnpm why --json` now includes the declared range of each dependent in a `requires` field [pnpm/pnpm#18121](https://github.com/pnpm/pnpm/pull/18121).
