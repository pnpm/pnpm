---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/engine.runtime.node-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm dedupe` now keeps a locked runtime version, from `devEngines.runtime` or from a dependency's `engines.runtime`, when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).
