## 12.0.0-rc.5

### Minor Changes

- **Breaking change.** Dependency cycles are now broken canonically during peer resolution: the members of each cycle are ordered by package id, and the edges that close a cycle are always cut at the same place, no matter where the installation walks into the cycle from. Previously the cut depended on the walk path, so installing the same dependencies could produce different lockfiles depending on importer order or resolution order [#13846](https://github.com/pnpm/pnpm/issues/13846), and a peer-resolution verdict computed for one occurrence of a cyclic package could be wrongly reused at another [#13865](https://github.com/pnpm/pnpm/issues/13865).

  With canonical cycle breaking the lockfile is a pure function of the dependency graph: repeated installs, reordered importers, and reordered dependencies all produce byte-identical lockfiles. Peer dependencies of packages inside a cycle keep nearest-wins resolution along the canonical order, and a dependency edge that closes a cycle references an occurrence of its target resolved at the importer level. On large cycle-heavy workspaces peer resolution is 2–3× faster, uses about 25% less memory, and produces a substantially smaller lockfile (fewer redundant peer variants).

  Existing lockfiles keep working: headless (`--frozen-lockfile`) installs consume them unchanged, and installs that skip resolution leave them untouched. The first install that actually re-resolves (for example after a dependency change) re-keys walk-order-dependent peer variants of cyclic packages once.

### Patch Changes

- Auto-installed peer dependencies are no longer resolved to their lowest satisfying versions under `resolutionMode: lowest-direct` or `time-based`. A hoisted peer is not a dependency the project declares, so it resolves like a transitive dependency — to the highest version satisfying the peer range (under `time-based`, the highest within the publish-date cutoff) [#13871](https://github.com/pnpm/pnpm/pull/13871).

- The resolved dependency graph and lockfile no longer depend on the order in which workspace projects are listed or discovered: importers are processed in project-id order, so reordering the `packages` globs in `pnpm-workspace.yaml` (or any other change to project listing order) produces a byte-identical lockfile [#13846](https://github.com/pnpm/pnpm/issues/13846). This also makes auto-installed peer placement, deprecation-warning attribution, and cycle back-edge bindings a function of the project set alone.

- Fixed non-deterministic lockfiles on cold installs of projects with cyclic peer dependencies: resolved peer variants could silently drop from the lockfile depending on traversal order [#13846](https://github.com/pnpm/pnpm/issues/13846), [#13865](https://github.com/pnpm/pnpm/issues/13865).

- A lockfile entry whose resolution is unchanged no longer loses its recorded `deprecated` marker when a registry serves the package's metadata inconsistently — re-resolving to the same version keeps the deprecation instead of silently dropping the line [#13846](https://github.com/pnpm/pnpm/issues/13846).

- `pnpm update` now writes the new version range back to `package.json` (and to the `catalog:` entry a dependency points at), instead of only updating the lockfile [#13879](https://github.com/pnpm/pnpm/issues/13879). The range operator the dependency already declared is preserved, and a dependency declared through a dist-tag (`"foo": "latest"`) keeps tracking the tag under both `pnpm update` and `pnpm update --latest`.
