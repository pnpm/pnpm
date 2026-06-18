---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fix unrelated lockfile churn for transitive dependencies. Previously, adding a new dependency that transitively pulled in a different version of a package already present in the lockfile could cause unrelated transitive references to that package to flip to the new version, even when the existing resolution still satisfied the wanted range. Re-resolution is now only forced for transitive dependencies whose alias was directly added/changed at the importer level [#11456](https://github.com/pnpm/pnpm/issues/11456).
