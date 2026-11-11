---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm dedupe` now keeps the locked `devEngines.runtime` version when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).
