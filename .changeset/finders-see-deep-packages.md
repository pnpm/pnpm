---
"pnpm": patch
---

`pnpm list --find-by` and `pnpm why --find-by` now offer their finders every package, however deep it sits in the dependency graph. They skipped a package more than 256 levels below the importer along the path the lookup took.
