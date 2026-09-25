---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

pnpm no longer writes a package's legacy array-form `engines`, such as `["node >= 0.8"]`, to the lockfile. It was recorded as an object keyed by index, such as `{'0': node >= 0.8}` [#4518](https://github.com/pnpm/pnpm/issues/4518).
