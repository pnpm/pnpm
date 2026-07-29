## 1102.3.7

### Patch Changes

- Speed up installs after compatible catalog or direct dependency range changes by retaining the locked version without resolving the dependency graph again.

- Speed up installs after safe override changes by reusing unambiguous compatible dependency resolutions, pruning obsolete dependencies, applying independent replacements and removals together, and handling parent-scoped `"-"` overrides without full lockfile resolution.

- `overrides` now also govern peers that pnpm auto-installs. Previously an override only rewrote dependencies declared in a manifest, so a peer nobody declares — installed because `autoInstallPeers` is on — resolved against its declared peer range and could bring in a second copy of the very package the override pinned. For example, with `overrides: { react: npm:react@19.2.0 }` and a lone `lucide-react` dependency, pnpm installed `react@18.3.1`; it now installs the pinned `react@19.2.0` [#13320](https://github.com/pnpm/pnpm/issues/13320).

- Installs through a pnpr server now apply the project's whole verification policy. `minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, and `trustLockfile` were ignored, so excluded packages were still held back and a lockfile containing them could be rejected.

  `trustPolicy: no-downgrade` no longer fails with `TRUST_POLICY_INCOMPATIBLE_WITH_PNPR` when a pnpr server is configured.

  `--frozen-lockfile` and `--no-prefer-frozen-lockfile` are now honored on the pnpr path, instead of resolving and rewriting the lockfile anyway. Since `frozenLockfile` defaults to `true` on CI, a CI install through a pnpr server now fails on an out-of-date lockfile rather than updating it.

- Workspace installs through a pnpr server no longer crash with `Cannot read properties of undefined (reading 'filter')` after linking, when `minimumReleaseAge` is active [#13275](https://github.com/pnpm/pnpm/issues/13275).

- Fixed `pnpm dedupe` updating valid catalog resolutions when another matching version exists in the lockfile.

- Restored the store block a first install prints, naming how packages were materialized and where the stores live [#13315](https://github.com/pnpm/pnpm/issues/13315):

  ```text
  Packages are hard linked from the content-addressable store to the virtual store.
    Content-addressable store is at: ~/.local/share/pnpm/store/v11
    Virtual store is at:             node_modules/.pnpm
  ```

- Prevented `pnpm dedupe --check` from removing an incompatible `node_modules` directory.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.23
  - @pnpm/bins.remover@1100.0.17
  - @pnpm/building.after-install@1102.0.13
  - @pnpm/building.during-install@1102.0.12
  - @pnpm/building.policy@1100.0.16
  - @pnpm/config.normalize-registries@1100.0.12
  - @pnpm/config.version-policy@1100.1.10
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.graph-hasher@1100.2.13
  - @pnpm/deps.path@1100.0.12
  - @pnpm/exec.lifecycle@1100.1.9
  - @pnpm/fs.symlink-dependency@1100.0.15
  - @pnpm/hooks.read-package-hook@1100.2.0
  - @pnpm/hooks.types@1100.2.4
  - @pnpm/installing.context@1100.0.29
  - @pnpm/installing.deps-resolver@1100.4.0
  - @pnpm/installing.deps-restorer@1102.2.0
  - @pnpm/installing.linking.direct-dep-linker@1100.0.15
  - @pnpm/installing.linking.hoist@1100.0.23
  - @pnpm/installing.linking.modules-cleaner@1100.1.16
  - @pnpm/installing.modules-yaml@1100.0.13
  - @pnpm/installing.package-requester@1102.1.7
  - @pnpm/lockfile.filtering@1100.2.0
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.preferred-versions@1100.0.26
  - @pnpm/lockfile.pruner@1100.0.17
  - @pnpm/lockfile.settings-checker@1100.1.8
  - @pnpm/lockfile.to-pnp@1100.1.10
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/lockfile.verification@1100.0.29
  - @pnpm/lockfile.walker@1100.0.17
  - @pnpm/network.auth-header@1101.1.7
  - @pnpm/patching.config@1100.0.13
  - @pnpm/pkg-manifest.utils@1100.2.13
  - @pnpm/pnpr.client@1.3.8
  - @pnpm/resolving.resolver-base@1100.5.5
  - @pnpm/store.controller-types@1100.1.11
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.project-manifest-reader@1100.0.21
