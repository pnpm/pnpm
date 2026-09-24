---
"pacquet": patch
---

`autoDedupe` and `pnpm dedupe` now move transitive dependencies to the version a `catalog:` dependency pins, as they already did for versions written directly in `package.json`. Previously they could move those dependencies to a higher version and keep both versions in the lockfile.
