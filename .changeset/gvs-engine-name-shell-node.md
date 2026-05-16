---
"@pnpm/deps.graph-hasher": patch
"@pnpm/engine.runtime.system-node-version": minor
"pnpm": patch
---

**fix**: anchor the side-effects-cache key and global-virtual-store hash to the script-runner Node version, not pnpm's own runtime.

`ENGINE_NAME` (the `<platform>;<arch>;node<major>` prefix used as the side-effects-cache key and the engine portion of the GVS hash) was computed from `process.version` — the Node that runs pnpm itself. For pnpm distributed via `@pnpm/exe` this is the Node embedded in the SEA bundle, not the `node` on the user's `PATH` that actually runs lifecycle scripts. Two pnpm installations on the same machine (one SEA, one npm-package) therefore disagreed on the cache key, partitioning the side-effects cache and the global virtual store across two Node majors even though both installs would run scripts on the same `node`.

Two changes:

- `@pnpm/engine.runtime.system-node-version` now exports `engineName()`. It uses `getSystemNodeVersion()` — which already prefers `node --version` from `PATH` over `process.version` when running inside a SEA bundle — so the value tracks the script-runner Node rather than the runner Node.
- `@pnpm/deps.graph-hasher`'s `calcDepState` and `calcGraphNodeHash` now call `engineName()` instead of importing the legacy `ENGINE_NAME` constant from `@pnpm/constants`. The constant in `@pnpm/constants` is unchanged so external consumers and existing tests keep working; in non-SEA contexts the two values are identical.

On upgrade, SEA-pnpm users will see a one-time GVS slot churn: packages that previously hashed under the embedded-Node major (e.g. `node26`) now hash under the shell-Node major (e.g. `node24`), matching what pacquet, the npm-published `pnpm` package, and any other pnpm-compatible tool already produce. Old slots become prune-eligible.
