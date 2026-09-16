---
"pacquet": patch
---

`pnpm install` now generates a Cargo lockfile when a crate version it considers depends on a release the registry carries only as yanked. pnpm rules that version out and resolves the rest of the graph. Resolution failed with an error such as `no non-yanked version of napi-build satisfies ^3.0.0-beta` [pnpm/pnpm#14952](https://github.com/pnpm/pnpm/issues/14952).
