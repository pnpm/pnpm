---
"@pnpm/lockfile.preferred-versions": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm dedupe` now moves transitive dependencies to the version a `catalog:` dependency pins, as it already did for versions written directly in `package.json`.
