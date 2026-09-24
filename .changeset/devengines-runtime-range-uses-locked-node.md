---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now checks `engines.node` against the Node.js version locked for `devEngines.runtime` when the runtime is downloaded. Optional dependencies that the locked Node.js supports are no longer skipped when `devEngines.runtime` declares a range [pnpm/pnpm#14628](https://github.com/pnpm/pnpm/issues/14628).
