# supi

## 0.43.18

### Patch Changes

- @pnpm/headless@14.5.9
- @pnpm/package-requester@12.2.0
- @pnpm/resolve-dependencies@18.2.1

## 0.43.17

### Patch Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.
- Updated dependencies [8698a7060]
  - @pnpm/package-requester@12.2.0
  - @pnpm/resolve-dependencies@18.2.0
  - @pnpm/resolver-base@7.1.0
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/lockfile-to-pnp@0.3.9
  - @pnpm/headless@14.5.8
  - @pnpm/lockfile-utils@2.0.20
  - @pnpm/build-modules@5.2.5
  - @pnpm/modules-cleaner@10.0.16
  - @pnpm/filter-lockfile@4.0.14
  - @pnpm/hoist@4.0.20

## 0.43.16

### Patch Changes

- @pnpm/resolve-dependencies@18.1.4
- @pnpm/lockfile-to-pnp@0.3.8
- @pnpm/headless@14.5.7
- @pnpm/package-requester@12.1.4

## 0.43.15

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/resolve-dependencies@18.1.3
  - @pnpm/filter-lockfile@4.0.13
  - @pnpm/get-context@3.3.1
  - @pnpm/headless@14.5.6
  - @pnpm/link-bins@5.3.20
  - @pnpm/lockfile-file@3.1.1
  - @pnpm/manifest-utils@1.1.4
  - @pnpm/read-package-json@3.1.8
  - @pnpm/read-project-manifest@1.1.5
  - @pnpm/package-requester@12.1.4
  - @pnpm/lockfile-to-pnp@0.3.7
  - @pnpm/modules-cleaner@10.0.15
  - @pnpm/build-modules@5.2.4
  - @pnpm/hoist@4.0.19
  - @pnpm/lifecycle@9.6.2
  - @pnpm/remove-bins@1.0.9

## 0.43.14

### Patch Changes

- Updated dependencies [3776b5a52]
- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0
  - @pnpm/get-context@3.3.0
  - @pnpm/headless@14.5.5
  - @pnpm/lockfile-to-pnp@0.3.6

## 0.43.13

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/modules-yaml@8.0.5
  - @pnpm/get-context@3.2.11
  - @pnpm/headless@14.5.4
  - @pnpm/lockfile-to-pnp@0.3.5
  - @pnpm/read-project-manifest@1.1.4
  - @pnpm/link-bins@5.3.19
  - @pnpm/build-modules@5.2.3
  - @pnpm/hoist@4.0.18
  - @pnpm/package-requester@12.1.3

## 0.43.12

### Patch Changes

- c4ec56eeb: Don't ignore the "overrides" field when install/update doesn't include the root project.
- Updated dependencies [39142e2ad]
- Updated dependencies [60e01bd1d]
- Updated dependencies [aa6bc4f95]
  - dependency-path@5.0.6
  - @pnpm/resolve-dependencies@18.1.2
  - @pnpm/lockfile-to-pnp@0.3.4
  - @pnpm/lockfile-file@3.0.17
  - @pnpm/filter-lockfile@4.0.12
  - @pnpm/headless@14.5.3
  - @pnpm/hoist@4.0.17
  - @pnpm/lockfile-utils@2.0.19
  - @pnpm/lockfile-walker@3.0.7
  - @pnpm/modules-cleaner@10.0.14
  - @pnpm/prune-lockfile@2.0.17
  - @pnpm/get-context@3.2.10
  - @pnpm/read-project-manifest@1.1.3
  - @pnpm/link-bins@5.3.18
  - @pnpm/package-requester@12.1.3
  - @pnpm/build-modules@5.2.2

## 0.43.11

### Patch Changes

- @pnpm/package-requester@12.1.3
- @pnpm/headless@14.5.2

## 0.43.10

### Patch Changes

- b5d694e7f: Use pnpm.overrides instead of resolutions. Still support resolutions for partial compatibility with Yarn and for avoiding a breaking change.
- c03a2b2cb: Allow to specify the overriden dependency's parent package.

  For example, if `foo` should be overriden only in dependencies of bar v2, this configuration may be used:

  ```json
  {
    ...
    "pnpm": {
      "overriden": {
        "bar@2>foo": "1.0.0"
      }
    }
  }
  ```

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/filter-lockfile@4.0.11
  - @pnpm/hoist@4.0.16
  - @pnpm/lockfile-file@3.0.16
  - @pnpm/lockfile-utils@2.0.18
  - @pnpm/lockfile-walker@3.0.6
  - @pnpm/modules-cleaner@10.0.13
  - @pnpm/prune-lockfile@2.0.16
  - @pnpm/resolve-dependencies@18.1.1
  - @pnpm/build-modules@5.2.1
  - @pnpm/core-loggers@5.0.2
  - dependency-path@5.0.5
  - @pnpm/get-context@3.2.9
  - @pnpm/headless@14.5.1
  - @pnpm/lifecycle@9.6.1
  - @pnpm/link-bins@5.3.17
  - @pnpm/lockfile-to-pnp@0.3.3
  - @pnpm/manifest-utils@1.1.3
  - @pnpm/modules-yaml@8.0.4
  - @pnpm/normalize-registries@1.0.5
  - @pnpm/package-requester@12.1.2
  - @pnpm/read-package-json@3.1.7
  - @pnpm/read-project-manifest@1.1.2
  - @pnpm/remove-bins@1.0.8
  - @pnpm/resolver-base@7.0.5
  - @pnpm/store-controller-types@9.1.2
  - @pnpm/symlink-dependency@3.0.12

## 0.43.9

### Patch Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.
- Updated dependencies [50b360ec1]
  - @pnpm/build-modules@5.2.0
  - @pnpm/headless@14.5.0
  - @pnpm/lifecycle@9.6.0
  - @pnpm/lockfile-to-pnp@0.3.2

## 0.43.8

### Patch Changes

- d54043ee4: A resolutions field in the root project's manifest may be used to override the version ranges in dependencies of dependencies.
- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
- Updated dependencies [fcdad632f]
- Updated dependencies [d54043ee4]
- Updated dependencies [212671848]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/resolve-dependencies@18.1.0
  - @pnpm/read-package-json@3.1.6
  - @pnpm/filter-lockfile@4.0.10
  - @pnpm/hoist@4.0.15
  - @pnpm/lockfile-file@3.0.15
  - @pnpm/lockfile-utils@2.0.17
  - @pnpm/lockfile-walker@3.0.5
  - @pnpm/modules-cleaner@10.0.12
  - @pnpm/prune-lockfile@2.0.15
  - @pnpm/build-modules@5.1.2
  - @pnpm/core-loggers@5.0.1
  - dependency-path@5.0.4
  - @pnpm/get-context@3.2.8
  - @pnpm/headless@14.4.2
  - @pnpm/lifecycle@9.5.1
  - @pnpm/link-bins@5.3.16
  - @pnpm/lockfile-to-pnp@0.3.1
  - @pnpm/manifest-utils@1.1.2
  - @pnpm/modules-yaml@8.0.3
  - @pnpm/normalize-registries@1.0.4
  - @pnpm/package-requester@12.1.1
  - @pnpm/read-project-manifest@1.1.1
  - @pnpm/remove-bins@1.0.7
  - @pnpm/resolver-base@7.0.4
  - @pnpm/store-controller-types@9.1.1
  - @pnpm/symlink-dependency@3.0.11

## 0.43.7

### Patch Changes

- Updated dependencies [fb863fae4]
  - @pnpm/link-bins@5.3.15
  - @pnpm/build-modules@5.1.1
  - @pnpm/headless@14.4.1
  - @pnpm/hoist@4.0.14

## 0.43.6

### Patch Changes

- Updated dependencies [4241bc148]
- Updated dependencies [bde7cd164]
- Updated dependencies [9f003e94f]
- Updated dependencies [e8dcc42d5]
- Updated dependencies [c6eaf01c9]
  - @pnpm/resolve-dependencies@18.0.6

## 0.43.5

### Patch Changes

- ddd98dd74: The lockfile should be correctly updated when a direct dependency that has peer dependencies has a new version specifier in `package.json`.

  For instance, `jest@26` has `cascade@2` in its peer dependencies. So `pnpm install` will scope Jest to some version of cascade. This is how it will look like in `pnpm-lock.yaml`:

  ```yaml
  dependencies:
    canvas: 2.6.0
    jest: 26.4.0_canvas@2.6.0
  ```

  If the version specifier of Jest gets changed in the `package.json` to `26.5.0`, the next time `pnpm install` is executed, the lockfile should be changed to this:

  ```yaml
  dependencies:
    canvas: 2.6.0
    jest: 26.5.0_canvas@2.6.0
  ```

  Prior to this fix, after the update, Jest was not scoped with canvas, so the lockfile was incorrectly updated to the following:

  ```yaml
  dependencies:
    canvas: 2.6.0
    jest: 26.5.0
  ```

  Related issue: [#2919](https://github.com/pnpm/pnpm/issues/2919).
  Related PR: [#2920](https://github.com/pnpm/pnpm/pull/2920).

- f591fdeeb: New option added: `enablePnp`. When enablePnp is true, a `.pnp.js` file is generated.
- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
- Updated dependencies [ddd98dd74]
- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
  - @pnpm/build-modules@5.1.0
  - @pnpm/lifecycle@9.5.0
  - @pnpm/resolve-dependencies@18.0.5
  - @pnpm/headless@14.4.0
  - @pnpm/lockfile-to-pnp@0.3.0
  - @pnpm/package-requester@12.1.0

## 0.43.4

### Patch Changes

- fb92e9f88: Perform less filesystem operations during the creation of bin files of direct dependencies.
- Updated dependencies [fb92e9f88]
- Updated dependencies [2762781cc]
- Updated dependencies [51311d3ba]
- Updated dependencies [fb92e9f88]
  - @pnpm/headless@14.3.1
  - @pnpm/read-project-manifest@1.1.0
  - @pnpm/link-bins@5.3.14
  - @pnpm/build-modules@5.0.19
  - @pnpm/hoist@4.0.13
  - @pnpm/package-requester@12.1.0

## 0.43.3

### Patch Changes

- 95ad9cafa: Install should fail if there are references to a prunned workspace project.

## 0.43.2

### Patch Changes

- 74914c178: New experimental option added for installing node_modules w/o symlinks.
- Updated dependencies [74914c178]
  - @pnpm/headless@14.3.0

## 0.43.1

### Patch Changes

- 9e774ae20: When a package is both a dev dependency and a prod dependency, the package should be linked when installing prod dependencies only. This was an issue only when a lockfile was not present during installation.
- Updated dependencies [203e65ac8]
- Updated dependencies [203e65ac8]
  - @pnpm/build-modules@5.0.18
  - @pnpm/lifecycle@9.4.0
  - @pnpm/resolve-dependencies@18.0.4
  - @pnpm/headless@14.2.2
  - @pnpm/package-requester@12.1.0

## 0.43.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

### Patch Changes

- Updated dependencies [23cf3c88b]
- Updated dependencies [ac3042858]
  - @pnpm/lifecycle@9.3.0
  - @pnpm/get-context@3.2.7
  - @pnpm/build-modules@5.0.17
  - @pnpm/headless@14.2.1

## 0.42.0

### Minor Changes

- 40a9e1f3f: Create the module dirs of dependencies before importing them and linking their dependencies.

### Patch Changes

- Updated dependencies [40a9e1f3f]
- Updated dependencies [0a6544043]
  - @pnpm/headless@14.2.0
  - @pnpm/package-requester@12.1.0
  - @pnpm/store-controller-types@9.1.0
  - @pnpm/build-modules@5.0.16
  - @pnpm/modules-cleaner@10.0.11
  - @pnpm/resolve-dependencies@18.0.3

## 0.41.31

### Patch Changes

- @pnpm/headless@14.1.0

## 0.41.30

### Patch Changes

- @pnpm/headless@14.1.0

## 0.41.29

### Patch Changes

- 86cd72de3: After a package is linked, copied, or cloned to the virtual store, a progress log is logged with the `imported` status.
- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/headless@14.1.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/build-modules@5.0.15
  - @pnpm/get-context@3.2.6
  - @pnpm/lifecycle@9.2.5
  - @pnpm/manifest-utils@1.1.1
  - @pnpm/modules-cleaner@10.0.10
  - @pnpm/package-requester@12.0.13
  - @pnpm/remove-bins@1.0.6
  - @pnpm/resolve-dependencies@18.0.2
  - @pnpm/symlink-dependency@3.0.10
  - @pnpm/filter-lockfile@4.0.9
  - @pnpm/hoist@4.0.12

## 0.41.28

### Patch Changes

- 968c26470: Report an info log instead of a warning when some binaries cannot be linked.
- Updated dependencies [968c26470]
  - @pnpm/headless@14.0.20
  - @pnpm/hoist@4.0.11
  - @pnpm/package-requester@12.0.12
  - @pnpm/resolve-dependencies@18.0.1

## 0.41.27

### Patch Changes

- 5a3420ee5: In some rare cases, `pnpm install --no-prefer-frozen-lockfile` didn't link the direct dependencies to the root `node_modules`. This was happening when the direct dependency was also resolving some peer dependencies.
- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
  - @pnpm/manifest-utils@1.1.0
  - @pnpm/resolve-dependencies@18.0.0

## 0.41.26

### Patch Changes

- 11dea936a: Fixing a regression that was shipped with supi v0.41.22. Cyclic dependencies that have peer dependencies are not symlinked to the root of node_modules, when they are direct dependencies.
- Updated dependencies [9d9456442]
- Updated dependencies [501efdabd]
- Updated dependencies [501efdabd]
- Updated dependencies [a43c12afe]
- Updated dependencies [501efdabd]
  - @pnpm/resolve-dependencies@17.0.0
  - @pnpm/package-requester@12.0.12
  - @pnpm/headless@14.0.19

## 0.41.25

### Patch Changes

- c4165dccb: Always try to resolve optional peer dependencies. Fixes a regression introduced in pnpm v5.5.8

## 0.41.24

### Patch Changes

- c7e856fac: Cache the already resolved peer dependencies to make peers resolution faster and consume less memory.

## 0.41.23

### Patch Changes

- 8242401c7: Ignore non-array bundle\[d]Dependencies fields. Fixes a regression caused by https://github.com/pnpm/pnpm/commit/5322cf9b39f637536aa4775aa64dd4e9a4156d8a
- Updated dependencies [8242401c7]
  - @pnpm/resolve-dependencies@16.1.5

## 0.41.22

### Patch Changes

- 8351fce25: Cache the already resolved peer dependencies to make peers resolution faster and consume less memory.
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/filter-lockfile@4.0.8
  - @pnpm/get-context@3.2.5
  - @pnpm/headless@14.0.18
  - @pnpm/link-bins@5.3.13
  - @pnpm/lockfile-file@3.0.14
  - @pnpm/read-package-json@3.1.5
  - @pnpm/read-project-manifest@1.0.13
  - @pnpm/resolve-dependencies@16.1.4
  - @pnpm/modules-cleaner@10.0.9
  - @pnpm/build-modules@5.0.14
  - @pnpm/hoist@4.0.10
  - @pnpm/lifecycle@9.2.4
  - @pnpm/package-requester@12.0.11
  - @pnpm/remove-bins@1.0.5

## 0.41.21

### Patch Changes

- 83e2e6879: When updating specs in the lockfile, read the specs from the manifest in the right order: optionalDependencies > dependencies > devDependencies.

## 0.41.20

### Patch Changes

- Updated dependencies [9f5803187]
- Updated dependencies [9550b0505]
- Updated dependencies [972864e0d]
  - @pnpm/read-package-json@3.1.4
  - @pnpm/lockfile-file@3.0.13
  - @pnpm/get-context@3.2.4
  - @pnpm/headless@14.0.17
  - @pnpm/package-requester@12.0.10
  - @pnpm/build-modules@5.0.13
  - @pnpm/lifecycle@9.2.3
  - @pnpm/link-bins@5.3.12
  - @pnpm/remove-bins@1.0.4
  - @pnpm/resolve-dependencies@16.1.3
  - @pnpm/hoist@4.0.9
  - @pnpm/modules-cleaner@10.0.8

## 0.41.19

### Patch Changes

- Updated dependencies [51086e6e4]
- Updated dependencies [6d480dd7a]
  - @pnpm/get-context@3.2.3
  - @pnpm/error@1.3.0
  - @pnpm/package-requester@12.0.9
  - @pnpm/filter-lockfile@4.0.7
  - @pnpm/headless@14.0.16
  - @pnpm/link-bins@5.3.11
  - @pnpm/lockfile-file@3.0.12
  - @pnpm/read-project-manifest@1.0.12
  - @pnpm/resolve-dependencies@16.1.2
  - @pnpm/modules-cleaner@10.0.7
  - @pnpm/build-modules@5.0.12
  - @pnpm/hoist@4.0.8

## 0.41.18

### Patch Changes

- 9b90591e4: The contents of a modified local tarball dependency should be reunpacked on install.
- Updated dependencies [400f41976]
  - @pnpm/headless@14.0.15

## 0.41.17

### Patch Changes

- @pnpm/read-project-manifest@1.0.11
- @pnpm/headless@14.0.14
- @pnpm/link-bins@5.3.10
- @pnpm/build-modules@5.0.11
- @pnpm/hoist@4.0.7
- @pnpm/package-requester@12.0.8

## 0.41.16

### Patch Changes

- 0a8ff3ad3: Don't fail when installing a dependency with a trailing @.
- Updated dependencies [3bd3253e3]
- Updated dependencies [24af41f20]
  - @pnpm/read-project-manifest@1.0.10
  - @pnpm/read-modules-dir@2.0.3
  - @pnpm/headless@14.0.13
  - @pnpm/link-bins@5.3.9
  - @pnpm/modules-cleaner@10.0.6
  - @pnpm/build-modules@5.0.10
  - @pnpm/hoist@4.0.6
  - @pnpm/package-requester@12.0.8

## 0.41.15

### Patch Changes

- 103ad7487: fix lockfile not updated when remove dependency in project with readPackage hook
- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [1140ef721]
- Updated dependencies [a2ef8084f]
  - @pnpm/lockfile-utils@2.0.16
  - @pnpm/build-modules@5.0.9
  - dependency-path@5.0.3
  - @pnpm/filter-lockfile@4.0.6
  - @pnpm/get-context@3.2.2
  - @pnpm/headless@14.0.12
  - @pnpm/hoist@4.0.5
  - @pnpm/lifecycle@9.2.2
  - @pnpm/lockfile-walker@3.0.4
  - @pnpm/modules-cleaner@10.0.5
  - @pnpm/modules-yaml@8.0.2
  - @pnpm/package-requester@12.0.8
  - @pnpm/prune-lockfile@2.0.14
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/remove-bins@1.0.3
  - @pnpm/resolve-dependencies@16.1.1
  - @pnpm/link-bins@5.3.8

## 0.41.14

### Patch Changes

- Updated dependencies [25b425ca2]
  - @pnpm/get-context@3.2.1

## 0.41.13

### Patch Changes

- Updated dependencies [873f08b04]
- Updated dependencies [873f08b04]
  - @pnpm/prune-lockfile@2.0.13
  - @pnpm/headless@14.0.11

## 0.41.12

### Patch Changes

- 8c1cf25b7: New option added: updateMatching. updateMatching is a function that accepts a package name. It returns `true` if the specified package should be updated.
- Updated dependencies [8c1cf25b7]
  - @pnpm/resolve-dependencies@16.1.0

## 0.41.11

### Patch Changes

- a01626668: Changes that are made by the `readPackage` hook are not saved to the `package.json` files of projects.
- Updated dependencies [a01626668]
  - @pnpm/get-context@3.2.0

## 0.41.10

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/get-context@3.1.0
  - @pnpm/build-modules@5.0.8
  - @pnpm/headless@14.0.10
  - @pnpm/lifecycle@9.2.1
  - @pnpm/modules-cleaner@10.0.4
  - @pnpm/package-requester@12.0.7
  - @pnpm/remove-bins@1.0.2
  - @pnpm/resolve-dependencies@16.0.6
  - @pnpm/symlink-dependency@3.0.9
  - @pnpm/filter-lockfile@4.0.5
  - @pnpm/hoist@4.0.4

## 0.41.9

### Patch Changes

- @pnpm/resolve-dependencies@16.0.5
- @pnpm/headless@14.0.9
- @pnpm/package-requester@12.0.6

## 0.41.8

### Patch Changes

- @pnpm/headless@14.0.8
- @pnpm/package-requester@12.0.6

## 0.41.7

### Patch Changes

- 1d8ec7208: Don't fail if opts.reporter is a string instead of a function.
- Updated dependencies [7f25dad04]
- Updated dependencies [76aaead32]
- Updated dependencies [7f25dad04]
  - @pnpm/resolve-dependencies@16.0.4
  - @pnpm/lifecycle@9.2.0
  - @pnpm/prune-lockfile@2.0.12
  - @pnpm/build-modules@5.0.7
  - @pnpm/headless@14.0.7

## 0.41.6

### Patch Changes

- @pnpm/headless@14.0.6
- @pnpm/package-requester@12.0.6
- @pnpm/resolve-dependencies@16.0.3

## 0.41.5

### Patch Changes

- @pnpm/package-requester@12.0.6
- @pnpm/headless@14.0.5

## 0.41.4

### Patch Changes

- 220896511: Remove common-tags from dependencies.
- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/build-modules@5.0.6
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/filter-lockfile@4.0.4
  - @pnpm/get-context@3.0.1
  - @pnpm/headless@14.0.4
  - @pnpm/hoist@4.0.3
  - @pnpm/lifecycle@9.1.3
  - @pnpm/link-bins@5.3.7
  - @pnpm/lockfile-file@3.0.11
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/lockfile-walker@3.0.3
  - @pnpm/manifest-utils@1.0.3
  - @pnpm/modules-cleaner@10.0.3
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/normalize-registries@1.0.3
  - @pnpm/package-requester@12.0.6
  - @pnpm/prune-lockfile@2.0.11
  - @pnpm/read-package-json@3.1.3
  - @pnpm/read-project-manifest@1.0.9
  - @pnpm/remove-bins@1.0.1
  - @pnpm/resolve-dependencies@16.0.2
  - @pnpm/resolver-base@7.0.3
  - @pnpm/store-controller-types@8.0.2
  - @pnpm/symlink-dependency@3.0.8

## 0.41.3

### Patch Changes

- 57d08f303: Remove global bins when unlinking.
- Updated dependencies [57d08f303]
  - @pnpm/remove-bins@1.0.0
  - @pnpm/modules-cleaner@10.0.2
  - @pnpm/headless@14.0.3

## 0.41.2

### Patch Changes

- 17b598c18: Don't remove skipped optional dependencies from the current lockfile on partial installation.
- 1520e3d6f: Update graceful-fs to v4.2.4
  - @pnpm/package-requester@12.0.5
  - @pnpm/link-bins@5.3.6
  - @pnpm/modules-cleaner@10.0.1
  - @pnpm/headless@14.0.2
  - @pnpm/build-modules@5.0.5
  - @pnpm/hoist@4.0.2

## 0.41.1

### Patch Changes

- Updated dependencies [0a2f3ecc6]
  - @pnpm/hoist@4.0.1
  - @pnpm/headless@14.0.1

## 0.41.0

### Minor Changes

- 71a8c8ce3: `shamefullyHoist` replaced by `publicHoistPattern` and `forcePublicHoistPattern`.
- 71a8c8ce3: Breaking changes to the `node_modules/.modules.yaml` file:
  - `hoistedAliases` replaced with `hoistedDependencies`.
  - `shamefullyHoist` replaced with `publicHoistPattern`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [e1ca9fc13]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/hoist@4.0.0
  - @pnpm/link-bins@5.3.5
  - @pnpm/modules-cleaner@10.0.0
  - @pnpm/headless@14.0.0
  - @pnpm/get-context@3.0.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/build-modules@5.0.4
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/filter-lockfile@4.0.3
  - @pnpm/lifecycle@9.1.2
  - @pnpm/lockfile-file@3.0.10
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/lockfile-walker@3.0.2
  - @pnpm/manifest-utils@1.0.2
  - @pnpm/normalize-registries@1.0.2
  - @pnpm/package-requester@12.0.5
  - @pnpm/prune-lockfile@2.0.10
  - @pnpm/read-package-json@3.1.2
  - @pnpm/read-project-manifest@1.0.8
  - @pnpm/resolve-dependencies@16.0.1
  - @pnpm/resolver-base@7.0.2
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/symlink-dependency@3.0.7

## 0.40.1

### Patch Changes

- @pnpm/package-requester@12.0.4
- @pnpm/headless@13.0.6

## 0.40.0

### Minor Changes

- 41d92948b: It should be possible to install a tarball through a non-standard URL endpoint served via the registry domain.

  For instance, the configured registry is `https://registry.npm.taobao.org/`.
  It should be possible to run `pnpm add https://registry.npm.taobao.org/vue/download/vue-2.0.0.tgz`

### Patch Changes

- Updated dependencies [41d92948b]
- Updated dependencies [57c510f00]
- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/read-project-manifest@1.0.7
  - @pnpm/resolve-dependencies@16.0.0
  - @pnpm/filter-lockfile@4.0.2
  - @pnpm/headless@13.0.5
  - @pnpm/hoist@3.0.2
  - @pnpm/lockfile-utils@2.0.13
  - @pnpm/lockfile-walker@3.0.1
  - @pnpm/modules-cleaner@9.0.2
  - @pnpm/prune-lockfile@2.0.9
  - @pnpm/link-bins@5.3.4
  - @pnpm/build-modules@5.0.3
  - @pnpm/package-requester@12.0.3

## 0.39.10

### Patch Changes

- 0e7ec4533: Remove @pnpm/check-package from dependencies.
- 13630c659: Perform headless installation when dependencies should not be linked from the workspace, and they are not indeed linked from the workspace.
- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
- Updated dependencies [68d8dc68f]
  - @pnpm/build-modules@5.0.2
  - @pnpm/headless@13.0.4
  - @pnpm/lifecycle@9.1.1
  - @pnpm/package-requester@12.0.3
  - @pnpm/core-loggers@4.1.0
  - @pnpm/resolve-dependencies@15.1.2
  - @pnpm/get-context@2.1.2
  - @pnpm/modules-cleaner@9.0.1
  - @pnpm/symlink-dependency@3.0.6
  - @pnpm/filter-lockfile@4.0.1
  - @pnpm/hoist@3.0.1

## 0.39.9

### Patch Changes

- Updated dependencies [a203bc138]
  - @pnpm/package-requester@12.0.2
  - @pnpm/headless@13.0.3

## 0.39.8

### Patch Changes

- @pnpm/package-requester@12.0.1
- @pnpm/resolve-dependencies@15.1.1
- @pnpm/headless@13.0.2

## 0.39.7

### Patch Changes

- Updated dependencies [8094b2a62]
  - @pnpm/lifecycle@9.1.0
  - @pnpm/package-requester@12.0.1
  - @pnpm/build-modules@5.0.1
  - @pnpm/headless@13.0.1

## 0.39.6

### Patch Changes

- 2f9c7ca85: Fix a regression introduced in pnpm v5.0.0.
  Create correct lockfile when the package tarball is hosted not under the registry domain.
- 160975d62: This fixes a regression introduced in pnpm v5.0.0. Direct local tarball dependencies should always be reanalized on install.

## 0.39.5

### Patch Changes

- @pnpm/headless@13.0.0

## 0.39.4

### Patch Changes

- Updated dependencies [58c02009f]
  - @pnpm/get-context@2.1.1

## 0.39.3

### Patch Changes

- 71b0cb8fd: Subdependencies are not needlessly updated.

  Fixes a regression introduced by [cc8a3bd312ea1405a6c79b1d157f0f9ae1be07aa](https://github.com/pnpm/pnpm/commit/cc8a3bd312ea1405a6c79b1d157f0f9ae1be07aa).

- Updated dependencies [71b0cb8fd]
  - @pnpm/resolve-dependencies@15.1.0

## 0.39.2

### Patch Changes

- 327bfbf02: Fix current lockfile (the one at `node_modules/.pnpm/lock.yaml`) up-to-date check.
- Updated dependencies [327bfbf02]
  - @pnpm/get-context@2.1.0

## 0.39.1

### Patch Changes

- Updated dependencies [e2c4fdad5]
  - @pnpm/resolve-dependencies@15.0.1

## 0.39.0

### Minor Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 242cf8737: The `linkWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `linkWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 45fdcfde2: Locking is removed.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- 2e8ebabb2: Headless installation should be preferred when local dependencies that use aliases are up-to-date.
- cc8a3bd31: Installation on a non-up-to-date `node_modules`.
- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [0730bb938]
- Updated dependencies [ca9f50844]
- Updated dependencies [9596774f2]
- Updated dependencies [7179cc560]
- Updated dependencies [77bc9b510]
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [242cf8737]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [142f8caf7]
- Updated dependencies [da091c711]
- Updated dependencies [9b1b520d9]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [a7d20d927]
- Updated dependencies [42e6490d1]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [2485eaf60]
- Updated dependencies [64bae33c4]
- Updated dependencies [e11019b89]
- Updated dependencies [a5febb913]
- Updated dependencies [bb59db642]
- Updated dependencies [b47f9737a]
- Updated dependencies [802d145fc]
- Updated dependencies [f93583d52]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [c207d994f]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [4cc0ead24]
- Updated dependencies [471149e66]
- Updated dependencies [c25cccdad]
- Updated dependencies [42e6490d1]
- Updated dependencies [9fbb74ecb]
- Updated dependencies [e3990787a]
  - @pnpm/constants@4.0.0
  - @pnpm/headless@13.0.0
  - @pnpm/hoist@3.0.0
  - @pnpm/modules-cleaner@9.0.0
  - @pnpm/package-requester@12.0.0
  - @pnpm/resolve-dependencies@15.0.0
  - @pnpm/filter-lockfile@4.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/get-context@2.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/lockfile-walker@3.0.0
  - @pnpm/types@6.0.0
  - @pnpm/build-modules@5.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/error@1.2.1
  - @pnpm/link-bins@5.3.3
  - @pnpm/lockfile-file@3.0.9
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/manifest-utils@1.0.1
  - @pnpm/matcher@1.0.3
  - @pnpm/normalize-registries@1.0.1
  - @pnpm/parse-wanted-dependency@1.0.1
  - @pnpm/prune-lockfile@2.0.8
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/read-package-json@3.1.1
  - @pnpm/read-project-manifest@1.0.6
  - @pnpm/resolver-base@7.0.1
  - @pnpm/symlink-dependency@3.0.5

## 0.39.0-alpha.7

### Minor Changes

- 242cf8737: The `linkWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `linkWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- 45fdcfde2: Locking is removed.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.

### Patch Changes

- cc8a3bd31: Installation on a non-up-to-date `node_modules`.
- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- Updated dependencies [ca9f50844]
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [242cf8737]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [a7d20d927]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [2485eaf60]
- Updated dependencies [a5febb913]
- Updated dependencies [b47f9737a]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [c25cccdad]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/filter-lockfile@4.0.0-alpha.2
  - @pnpm/package-requester@12.0.0-alpha.5
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/resolve-dependencies@15.0.0-alpha.6
  - @pnpm/headless@13.0.0-alpha.5
  - @pnpm/hoist@3.0.0-alpha.2
  - @pnpm/modules-cleaner@9.0.0-alpha.5
  - @pnpm/build-modules@5.0.0-alpha.5
  - @pnpm/get-context@1.2.2-alpha.2
  - @pnpm/lockfile-file@3.0.9-alpha.2
  - @pnpm/prune-lockfile@2.0.8-alpha.2
  - @pnpm/lockfile-utils@2.0.12-alpha.1
  - @pnpm/lockfile-walker@2.0.3-alpha.1

## 0.39.0-alpha.6

### Minor Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [7179cc56]
- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
- Updated dependencies [4cc0ead2]
- Updated dependencies [471149e6]
- Updated dependencies [9fbb74ec]
- Updated dependencies [e3990787]
  - @pnpm/modules-cleaner@9.0.0-alpha.4
  - @pnpm/get-context@2.0.0-alpha.1
  - @pnpm/headless@13.0.0-alpha.4
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/resolve-dependencies@15.0.0-alpha.5
  - @pnpm/hoist@3.0.0-alpha.1
  - @pnpm/build-modules@5.0.0-alpha.4
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/filter-lockfile@3.2.3-alpha.1
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.1
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/lockfile-walker@2.0.3-alpha.0
  - @pnpm/manifest-utils@1.0.1-alpha.0
  - @pnpm/normalize-registries@1.0.1-alpha.0
  - @pnpm/prune-lockfile@2.0.8-alpha.1
  - @pnpm/read-package-json@3.1.1-alpha.0
  - @pnpm/read-project-manifest@1.0.6-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0
  - @pnpm/symlink-dependency@3.0.5-alpha.0

## 0.39.0-alpha.5

### Patch Changes

- Updated dependencies [0730bb938]
  - @pnpm/resolve-dependencies@14.4.5-alpha.4

## 0.39.0-alpha.4

### Minor Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/headless@13.0.0-alpha.3
  - @pnpm/hoist@3.0.0-alpha.0
  - @pnpm/modules-cleaner@9.0.0-alpha.3
  - @pnpm/package-requester@12.0.0-alpha.3
  - @pnpm/build-modules@4.1.15-alpha.3
  - @pnpm/filter-lockfile@3.2.3-alpha.0
  - @pnpm/get-context@1.2.2-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.0
  - @pnpm/prune-lockfile@2.0.8-alpha.0
  - @pnpm/resolve-dependencies@14.4.5-alpha.3

## 0.39.0-alpha.3

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
- Updated dependencies [64bae33c4]
- Updated dependencies [c207d994f]
- Updated dependencies [42e6490d1]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/build-modules@4.1.14-alpha.2
  - @pnpm/headless@12.2.2-alpha.2
  - @pnpm/modules-cleaner@8.0.17-alpha.2
  - @pnpm/resolve-dependencies@14.4.5-alpha.2

## 0.39.0-alpha.2

### Patch Changes

- 2e8ebabb2: Headless installation should be preferred when local dependencies that use aliases are up-to-date.

## 0.39.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- Updated dependencies [4f62d0383]
- Updated dependencies [f93583d52]
  - @pnpm/package-requester@12.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.1
  - @pnpm/headless@12.2.2-alpha.1
  - @pnpm/build-modules@4.1.14-alpha.1
  - @pnpm/modules-cleaner@8.0.17-alpha.1
  - @pnpm/resolve-dependencies@14.4.5-alpha.1

## 0.39.0-alpha.0

### Minor Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/headless@13.0.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.0
  - @pnpm/build-modules@4.1.14-alpha.0
  - @pnpm/modules-cleaner@8.0.17-alpha.0
  - @pnpm/resolve-dependencies@14.4.5-alpha.0

## 0.38.30

### Patch Changes

- 760cc6664: Headless installation should be preferred when local dependencies that use aliases are up-to-date.
- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0
  - @pnpm/build-modules@4.1.14
  - @pnpm/headless@12.2.2

## 0.38.29

### Patch Changes

- 907c63a48: Update symlink-dir to v4.
- 907c63a48: Update `@pnpm/store-path`.
- 907c63a48: Dependencies updated.
- 907c63a48: Dependencies updated.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
- 907c63a48: `pnpm update --no-save` does not update the specs in the `package.json` files.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/package-requester@11.0.6
  - @pnpm/symlink-dependency@3.0.4
  - @pnpm/headless@12.2.1
  - @pnpm/link-bins@5.3.2
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/matcher@1.0.2
  - @pnpm/get-context@1.2.1
  - @pnpm/filter-lockfile@3.2.2
  - @pnpm/lockfile-utils@2.0.11
  - @pnpm/modules-yaml@6.0.2
  - @pnpm/hoist@2.2.3
  - @pnpm/build-modules@4.1.13
  - @pnpm/modules-cleaner@8.0.16
  - @pnpm/resolve-dependencies@14.4.4
  - @pnpm/read-project-manifest@1.0.5
