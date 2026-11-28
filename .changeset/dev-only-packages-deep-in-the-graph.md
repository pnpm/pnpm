---
"pnpm": patch
---

`pnpm licenses` and `pnpm list` now tell dev-only packages from production ones however deep a package sits in the dependency graph. A package the lookup reached more than 256 levels deep was left unclassified, so `pnpm licenses` counted it as a production dependency.
