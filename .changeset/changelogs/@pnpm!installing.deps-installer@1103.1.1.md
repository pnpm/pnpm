## 1103.1.1

### Patch Changes

- Removing a dependency from `package.json` and reinstalling no longer re-resolves the dependency graph. The importer's entry is dropped from `pnpm-lock.yaml`, anything it made unreachable is pruned, and a catalog entry that loses its last referent is removed — all without registry access. Installs still fall back to a full resolution when a package that stays resolves a peer dependency through the removed one, since that would change the surviving package's entry rather than only prune.

- Changing a catalog entry to a different exact version no longer re-resolves the dependency graph. The package is replaced in `pnpm-lock.yaml` directly, reusing the same check the `pnpm.overrides` fast path applies: every locked dependency of the package must still satisfy the new version's manifest. Installs fall back to a full resolution when anything other than the catalog reaches the package — an importer that depends on it directly, or another package that depends on it — since the graph would then need both versions.

- Changing `autoInstallPeers`, `dedupePeers`, `peersSuffixMaxLength`, `excludeLinksFromLockfile`, or `injectWorkspacePackages` no longer re-resolves the dependency graph when the lockfile proves the setting cannot affect it: no package or project declares a peer dependency for the peer settings, and no project depends on a directory or on another workspace project for the link and injection settings. The new setting is recorded in `pnpm-lock.yaml` and the install proceeds from the existing resolution. Every other case still falls back to a full resolution.

- Adding, editing, or removing an entry in `patchedDependencies` no longer re-resolves the dependency graph. Resolution never reads a patch — it only records the patch file's hash against the package it matches — so the install now rewrites the affected entries in `pnpm-lock.yaml` and materializes the patched package from the store instead. Installs still fall back to a full resolution when the patched package is reachable as a peer dependency, and when the new configuration would leave a patch unused while `allowUnusedPatches` is off, so `ERR_PNPM_UNUSED_PATCH` is still reported.

- `pnpm fetch`, and any install run with `virtualStoreOnly`, no longer writes a `.pnp.cjs` loader under `nodeLinker: pnp`. These installs populate the virtual store without linking the project, so the loader would have claimed the project resolves out of a store it was never linked into. The importer links and `node_modules/.package-map.json` were already skipped; the PnP loader now follows the same rule.

- Prevent pnpm from removing project files when `modulesDir` resolves to the project root.

- Speed up installs after adding `ignoredOptionalDependencies` patterns by removing newly ignored optional dependencies and pruning packages that are no longer reachable without resolving the dependency graph again.

- `pnpm update` without saving no longer records a version that the manifest's range excludes. The kept range stays authoritative: a requested version outside it is skipped with a warning, and a requested range, a dist tag, or `--latest` resolves within it instead of past it. Previously each of these could write a lockfile entry that contradicted its own specifier, which the next `pnpm install --frozen-lockfile` rejected with `ERR_PNPM_OUTDATED_LOCKFILE` [#12764](https://github.com/pnpm/pnpm/issues/12764).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.26
  - @pnpm/building.after-install@1102.0.16
  - @pnpm/building.during-install@1102.0.15
  - @pnpm/deps.graph-hasher@1100.2.16
  - @pnpm/exec.lifecycle@1100.1.12
  - @pnpm/hooks.read-package-hook@1100.2.3
  - @pnpm/installing.context@1100.1.1
  - @pnpm/installing.deps-resolver@1101.1.1
  - @pnpm/installing.deps-restorer@1102.3.1
  - @pnpm/installing.linking.hoist@1100.0.26
  - @pnpm/lockfile.fs@1100.2.1
  - @pnpm/lockfile.settings-checker@1100.2.0
  - @pnpm/lockfile.to-pnp@1100.1.13
  - @pnpm/lockfile.verification@1100.0.32
  - @pnpm/patching.config@1100.1.0
  - @pnpm/pnpr.client@1.3.11
