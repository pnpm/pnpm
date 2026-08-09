## 1102.3.1

### Patch Changes

- An install that skips resolution because `pnpm-lock.yaml` is already up to date now reacts fully to packages the lockfile removed — for example after pulling a lockfile in which a dependency was deleted. The hoist layer is recomputed, so a package that became hoistable when a direct dependency was removed is hoisted, and `pendingBuilds` entries for removed packages are dropped instead of staying pending forever.

- `pnpm fetch`, and any install run with `virtualStoreOnly`, no longer writes a `.pnp.cjs` loader under `nodeLinker: pnp`. These installs populate the virtual store without linking the project, so the loader would have claimed the project resolves out of a store it was never linked into. The importer links and `node_modules/.package-map.json` were already skipped; the PnP loader now follows the same rule.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.26
  - @pnpm/building.during-install@1102.0.15
  - @pnpm/deps.graph-builder@1100.2.1
  - @pnpm/deps.graph-hasher@1100.2.16
  - @pnpm/exec.lifecycle@1100.1.12
  - @pnpm/installing.linking.hoist@1100.0.26
  - @pnpm/lockfile.fs@1100.2.1
  - @pnpm/lockfile.to-pnp@1100.1.13
  - @pnpm/patching.config@1100.1.0
