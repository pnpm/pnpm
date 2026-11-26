---
"pnpm": patch
---

`pnpm audit deprecations` now prints one block per deprecated package, with its message shown once and each deprecated version listed below it. Long dependency chains in `pnpm audit deprecations` and `pnpm audit engines` now show only the direct dependency and the two packages closest to the deprecated one.
