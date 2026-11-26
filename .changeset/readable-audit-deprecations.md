---
"pnpm": patch
---

`pnpm audit deprecations` now lists the deprecated versions of a package that share a deprecation message in one block, with the message shown once. Long dependency chains in `pnpm audit deprecations` and `pnpm audit engines` now show only the direct dependency and the two packages closest to the reported one.
