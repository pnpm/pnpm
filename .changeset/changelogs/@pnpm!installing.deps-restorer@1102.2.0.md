## 1102.2.0

### Minor Changes

- Fixed an installed optional dependency being left without one of its own required dependencies. When a package reached through `optionalDependencies` is installable on the current system but one of its regular `dependencies` is not, a lockfile-based install skipped that dependency and installed the parent anyway, so importing the parent failed with `MODULE_NOT_FOUND`. The dependency is now installed, and an install-check warning reports the incompatibility. A dependency is still only skipped when every path to it is optional, or when the package that pulls it in was itself skipped [#13286](https://github.com/pnpm/pnpm/issues/13286).

### Patch Changes

- Speed up installs after safe override changes by reusing unambiguous compatible dependency resolutions, pruning obsolete dependencies, applying independent replacements and removals together, and handling parent-scoped `"-"` overrides without full lockfile resolution.

- Restored the store block a first install prints, naming how packages were materialized and where the stores live [#13315](https://github.com/pnpm/pnpm/issues/13315):

  ```text
  Packages are hard linked from the content-addressable store to the virtual store.
    Content-addressable store is at: ~/.local/share/pnpm/store/v11
    Virtual store is at:             node_modules/.pnpm
  ```

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.23
  - @pnpm/building.during-install@1102.0.12
  - @pnpm/building.policy@1100.0.16
  - @pnpm/config.package-is-installable@1100.1.0
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.graph-builder@1100.1.0
  - @pnpm/deps.graph-hasher@1100.2.13
  - @pnpm/deps.path@1100.0.12
  - @pnpm/exec.lifecycle@1100.1.9
  - @pnpm/fs.symlink-dependency@1100.0.15
  - @pnpm/installing.linking.direct-dep-linker@1100.0.15
  - @pnpm/installing.linking.hoist@1100.0.23
  - @pnpm/installing.linking.modules-cleaner@1100.1.16
  - @pnpm/installing.linking.real-hoist@1100.1.10
  - @pnpm/installing.modules-yaml@1100.0.13
  - @pnpm/installing.package-requester@1102.1.7
  - @pnpm/lockfile.filtering@1100.2.0
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.to-pnp@1100.1.10
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/patching.config@1100.0.13
  - @pnpm/pkg-manifest.reader@1100.0.13
  - @pnpm/store.controller-types@1100.1.11
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.project-manifest-reader@1100.0.21
