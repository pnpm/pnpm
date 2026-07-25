---
"pacquet": patch
---

A deprecated package is reported once rather than once per workspace project that depends on it, and is no longer double-counted in the "deprecated subdependencies found" summary when it is also a direct dependency [#13322](https://github.com/pnpm/pnpm/issues/13322). Ignored build scripts are also listed with their `(patch_hash=…)` suffix, so two copies of a package that differ only by an applied patch are distinguishable.
