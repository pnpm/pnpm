# @pnpm/core

## 5.11.3

### Patch Changes

- Updated dependencies [0373af22e]
  - @pnpm/lockfile-file@5.3.4
  - @pnpm/resolve-dependencies@28.3.9
  - @pnpm/get-context@6.2.10
  - @pnpm/headless@18.7.1
  - @pnpm/lockfile-to-pnp@1.0.1
  - @pnpm/package-requester@19.0.2

## 5.11.2

### Patch Changes

- Updated dependencies [829b4d924]
  - @pnpm/resolve-dependencies@28.3.8
  - @pnpm/headless@18.7.0
  - @pnpm/package-requester@19.0.2

## 5.11.1

### Patch Changes

- Updated dependencies [53506c7ae]
  - @pnpm/resolve-dependencies@28.3.7
  - @pnpm/headless@18.7.0
  - @pnpm/package-requester@19.0.2

## 5.11.0

### Minor Changes

- 2aa22e4b1: Set `NODE_PATH` when `preferSymlinkedExecutables` is enabled.

### Patch Changes

- Updated dependencies [e3b5137d1]
- Updated dependencies [2aa22e4b1]
  - @pnpm/symlink-dependency@5.0.7
  - @pnpm/headless@18.7.0
  - @pnpm/hoist@6.2.9

## 5.10.3

### Patch Changes

- f4cc2d7b4: fix mergeGitBranchLockfiles when merged lockfile is up-to-date
- Updated dependencies [1beb1b4bd]
  - @pnpm/filter-lockfile@6.0.18
  - @pnpm/headless@18.6.5
  - @pnpm/modules-cleaner@12.0.21

## 5.10.2

### Patch Changes

- @pnpm/package-requester@19.0.2
- @pnpm/store-controller-types@14.1.1
- @pnpm/headless@18.6.4
- @pnpm/crypto.base32-hash@1.0.1
- @pnpm/link-bins@7.2.5

## 5.10.1

### Patch Changes

- 9faf0221d: Update Yarn dependencies.
- Updated dependencies [dbac0ca01]
- Updated dependencies [07bc24ad1]
- Updated dependencies [dbac0ca01]
- Updated dependencies [07bc24ad1]
- Updated dependencies [9faf0221d]
- Updated dependencies [054b4e062]
- Updated dependencies [071aa1842]
  - @pnpm/package-requester@19.0.1
  - @pnpm/link-bins@7.2.5
  - @pnpm/resolve-dependencies@28.3.6
  - @pnpm/read-package-json@6.0.8
  - @pnpm/headless@18.6.3
  - @pnpm/build-modules@9.3.6
  - @pnpm/hoist@6.2.8
  - @pnpm/lifecycle@13.1.7
  - @pnpm/remove-bins@3.0.9
  - @pnpm/modules-cleaner@12.0.20
  - @pnpm/crypto.base32-hash@1.0.1

## 5.10.0

### Minor Changes

- 5035fdae1: Add new `preResolution` hook.

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [5035fdae1]
- Updated dependencies [23984abd1]
- Updated dependencies [7a17f99ab]
  - @pnpm/package-requester@19.0.0
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/lockfile-to-pnp@1.0.0
  - @pnpm/resolver-base@9.1.0
  - @pnpm/headless@18.6.2
  - @pnpm/build-modules@9.3.5
  - @pnpm/lifecycle@13.1.6
  - @pnpm/modules-cleaner@12.0.19
  - @pnpm/resolve-dependencies@28.3.5
  - @pnpm/lockfile-utils@4.2.4
  - @pnpm/filter-lockfile@6.0.17
  - @pnpm/hoist@6.2.7
  - @pnpm/crypto.base32-hash@1.0.1
  - @pnpm/link-bins@7.2.4

## 5.9.1

### Patch Changes

- Updated dependencies [238a165a5]
- Updated dependencies [c191ca7bf]
- Updated dependencies [1e5482da4]
  - @pnpm/parse-wanted-dependency@3.0.2
  - @pnpm/package-requester@18.1.3
  - @pnpm/lockfile-file@5.3.3
  - @pnpm/resolve-dependencies@28.3.4
  - @pnpm/parse-overrides@2.0.3
  - @pnpm/headless@18.6.1
  - @pnpm/get-context@6.2.9
  - @pnpm/lockfile-to-pnp@0.5.27
  - @pnpm/crypto.base32-hash@1.0.1
  - @pnpm/link-bins@7.2.4

## 5.9.0

### Minor Changes

- 43cd6aaca: When `ignore-dep-scripts` is `true`, ignore scripts of dependencies but run the scripts of the project.
- 65c4260de: Support a new hook for passing a custom package importer to the store controller.
- 29a81598a: When `ignore-compatibility-db` is set to `true`, the [compatibility database](https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-extensions/sources/index.ts) will not be used to patch dependencies [#5132](https://github.com/pnpm/pnpm/issues/5132).

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [0321ca32a]
- Updated dependencies [39c040127]
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/resolve-dependencies@28.3.3
  - @pnpm/build-modules@9.3.4
  - @pnpm/read-project-manifest@3.0.9
  - @pnpm/headless@18.6.0
  - @pnpm/filter-lockfile@6.0.16
  - @pnpm/get-context@6.2.8
  - @pnpm/hoist@6.2.6
  - @pnpm/link-bins@7.2.4
  - @pnpm/lockfile-file@5.3.2
  - @pnpm/lockfile-to-pnp@0.5.26
  - @pnpm/lockfile-utils@4.2.3
  - @pnpm/lockfile-walker@5.0.13
  - @pnpm/modules-cleaner@12.0.18
  - @pnpm/package-requester@18.1.2
  - @pnpm/prune-lockfile@4.0.14
  - @pnpm/store-controller-types@14.1.0
  - @pnpm/lifecycle@13.1.5
  - @pnpm/crypto.base32-hash@1.0.1

## 5.8.4

### Patch Changes

- Updated dependencies [44544b493]
- Updated dependencies [c90798461]
  - @pnpm/lockfile-file@5.3.1
  - @pnpm/types@8.5.0
  - @pnpm/get-context@6.2.7
  - @pnpm/headless@18.5.5
  - @pnpm/lockfile-to-pnp@0.5.25
  - @pnpm/resolve-dependencies@28.3.2
  - @pnpm/build-modules@9.3.3
  - @pnpm/core-loggers@7.0.6
  - dependency-path@9.2.4
  - @pnpm/filter-lockfile@6.0.15
  - @pnpm/hoist@6.2.5
  - @pnpm/lifecycle@13.1.4
  - @pnpm/link-bins@7.2.3
  - @pnpm/lockfile-utils@4.2.2
  - @pnpm/lockfile-walker@5.0.12
  - @pnpm/manifest-utils@3.1.2
  - @pnpm/modules-cleaner@12.0.17
  - @pnpm/modules-yaml@10.0.6
  - @pnpm/normalize-registries@3.0.6
  - @pnpm/package-requester@18.1.1
  - @pnpm/prune-lockfile@4.0.13
  - @pnpm/read-package-json@6.0.7
  - @pnpm/read-project-manifest@3.0.8
  - @pnpm/remove-bins@3.0.8
  - @pnpm/resolver-base@9.0.6
  - @pnpm/store-controller-types@14.0.2
  - @pnpm/symlink-dependency@5.0.6
  - @pnpm/crypto.base32-hash@1.0.1

## 5.8.3

### Patch Changes

- c7d65fe7f: Don't incorrectly consider a lockfile out-of-date when `workspace:^` or `workspace:~` version specs are used in a workspace.
- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1
  - @pnpm/filter-lockfile@6.0.14
  - @pnpm/headless@18.5.4
  - @pnpm/hoist@6.2.4
  - @pnpm/lockfile-to-pnp@0.5.24
  - @pnpm/modules-cleaner@12.0.16
  - @pnpm/resolve-dependencies@28.3.1

## 5.8.2

### Patch Changes

- Updated dependencies [cac34ad69]
  - @pnpm/package-requester@18.1.0
  - @pnpm/lockfile-to-pnp@0.5.23
  - @pnpm/headless@18.5.3

## 5.8.1

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-file@5.3.0
  - @pnpm/lockfile-utils@4.2.0
  - @pnpm/resolve-dependencies@28.3.0
  - @pnpm/get-context@6.2.6
  - @pnpm/headless@18.5.2
  - @pnpm/lockfile-to-pnp@0.5.22
  - @pnpm/filter-lockfile@6.0.13
  - @pnpm/hoist@6.2.3
  - @pnpm/lockfile-walker@5.0.11
  - @pnpm/modules-cleaner@12.0.15
  - @pnpm/prune-lockfile@4.0.12
  - @pnpm/crypto.base32-hash@1.0.1
  - @pnpm/link-bins@7.2.2
  - @pnpm/package-requester@18.0.13

## 5.8.0

### Minor Changes

- 4fa1091c8: Add experimental lockfile format that should merge conflict less in the `importers` section. Enabled by setting the `use-inline-specifiers-lockfile-format = true` feature flag in `.npmrc`.

  If this feature flag is committed to a repo, we recommend setting the minimum allowed version of pnpm to this release in the `package.json` `engines` field. Once this is set, older pnpm versions will throw on invalid lockfile versions.

### Patch Changes

- Updated dependencies [01c5834bf]
- Updated dependencies [4fa1091c8]
  - @pnpm/read-project-manifest@3.0.7
  - @pnpm/lockfile-file@5.2.0
  - @pnpm/headless@18.5.1
  - @pnpm/link-bins@7.2.2
  - @pnpm/lockfile-to-pnp@0.5.21
  - @pnpm/get-context@6.2.5
  - @pnpm/resolve-dependencies@28.2.3
  - @pnpm/lifecycle@13.1.3
  - @pnpm/build-modules@9.3.2
  - @pnpm/hoist@6.2.2
  - @pnpm/package-requester@18.0.13

## 5.7.0

### Minor Changes

- 0569f1022: When `saveLockfile` is set to `false`, no changes to `pnpm-lock.yaml` are written to the filesystem.

### Patch Changes

- e3f4d131c: When `auto-install-peers` is set to `true`, automatically install direct peer dependencies [#5028](https://github.com/pnpm/pnpm/pull/5067).

  So if your project the next manifest:

  ```json
  {
    "dependencies": {
      "lodash": "^4.17.21"
    },
    "peerDependencies": {
      "react": "^18.2.0"
    }
  }
  ```

  pnpm will install both lodash and react as a regular dependencies.

- Updated dependencies [e3f4d131c]
- Updated dependencies [e3f4d131c]
- Updated dependencies [e3f4d131c]
  - @pnpm/resolve-dependencies@28.2.2
  - @pnpm/manifest-utils@3.1.1
  - @pnpm/headless@18.5.0
  - @pnpm/lockfile-utils@4.1.0
  - @pnpm/lockfile-to-pnp@0.5.20
  - @pnpm/link-bins@7.2.1
  - @pnpm/filter-lockfile@6.0.12
  - @pnpm/hoist@6.2.1
  - @pnpm/modules-cleaner@12.0.14
  - @pnpm/build-modules@9.3.1

## 5.6.0

### Minor Changes

- 28f000509: A new setting supported: `prefer-symlinked-executables`. When `true`, pnpm will create symlinks to executables in
  `node_modules/.bin` instead of command shims (but on POSIX systems only).

  This setting is `true` by default when `node-linker` is set to `hoisted`.

  Related issue: [#4782](https://github.com/pnpm/pnpm/issues/4782).

### Patch Changes

- 0ee3dfbe0: Don't print warnings about file verifications. Just print info messages instead.
- 406656f80: When `lockfile-include-tarball-url` is set to `true`, every entry in `pnpm-lock.yaml` will contain the full URL to the package's tarball [#5054](https://github.com/pnpm/pnpm/pull/5054).
- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/build-modules@9.3.0
  - @pnpm/headless@18.4.0
  - @pnpm/hoist@6.2.0
  - @pnpm/link-bins@7.2.0
  - @pnpm/resolve-dependencies@28.2.1
  - @pnpm/lockfile-to-pnp@0.5.19
  - @pnpm/package-requester@18.0.13

## 5.5.9

### Patch Changes

- @pnpm/lockfile-to-pnp@0.5.18
- @pnpm/headless@18.3.7

## 5.5.8

### Patch Changes

- d89bb43f2: Don't symlink the autoinstalled peer dependencies to the root of `node_modules` [#4988](https://github.com/pnpm/pnpm/issues/4988).

## 5.5.7

### Patch Changes

- ff7061929: `pnpm remove <pkg>` should not fail in a workspace that has patches [#4954](https://github.com/pnpm/pnpm/issues/4954#issuecomment-1172858634)
- Updated dependencies [f5621a42c]
- Updated dependencies [2bca856e0]
  - @pnpm/manifest-utils@3.1.0
  - @pnpm/resolve-dependencies@28.2.0
  - @pnpm/which-version-is-pinned@3.0.0
  - @pnpm/crypto.base32-hash@1.0.1
  - @pnpm/link-bins@7.1.7
  - dependency-path@9.2.3
  - @pnpm/build-modules@9.2.4
  - @pnpm/headless@18.3.6
  - @pnpm/hoist@6.1.9
  - @pnpm/filter-lockfile@6.0.11
  - @pnpm/lockfile-to-pnp@0.5.17
  - @pnpm/lockfile-utils@4.0.10
  - @pnpm/lockfile-walker@5.0.10
  - @pnpm/modules-cleaner@12.0.13
  - @pnpm/package-requester@18.0.13
  - @pnpm/prune-lockfile@4.0.11

## 5.5.6

### Patch Changes

- b55b3782d: Never skip lockfile resolution when the lockfile is not up-to-date and `--lockfile-only` is used. Even if `frozen-lockfile` is `true` [#4951](https://github.com/pnpm/pnpm/issues/4951).
- Updated dependencies [5e0e7f5db]
- Updated dependencies [ab684d77e]
  - @pnpm/resolve-dependencies@28.1.4
  - @pnpm/lockfile-file@5.1.4
  - @pnpm/get-context@6.2.4
  - @pnpm/headless@18.3.5
  - @pnpm/lockfile-to-pnp@0.5.16
  - @pnpm/package-requester@18.0.12

## 5.5.5

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- 42c1ea1c0: Update validate-npm-package-name to v4.
- c5fdc5f35: Update the compatibility database.
- Updated dependencies [5f643f23b]
- Updated dependencies [42c1ea1c0]
  - @pnpm/build-modules@9.2.3
  - @pnpm/filter-lockfile@6.0.10
  - @pnpm/get-context@6.2.3
  - @pnpm/headless@18.3.4
  - @pnpm/hoist@6.1.8
  - @pnpm/link-bins@7.1.6
  - @pnpm/lockfile-file@5.1.3
  - @pnpm/lockfile-to-pnp@0.5.15
  - @pnpm/lockfile-utils@4.0.9
  - @pnpm/lockfile-walker@5.0.9
  - @pnpm/modules-cleaner@12.0.12
  - @pnpm/package-requester@18.0.12
  - @pnpm/prune-lockfile@4.0.10
  - @pnpm/remove-bins@3.0.7
  - @pnpm/resolve-dependencies@28.1.3
  - @pnpm/parse-wanted-dependency@3.0.1
  - @pnpm/lifecycle@13.1.2
  - @pnpm/parse-overrides@2.0.2

## 5.5.4

### Patch Changes

- fc581d371: Don't fail when the patched package appears multiple times in the dependency graph [#4938](https://github.com/pnpm/pnpm/issues/4938).
- Updated dependencies [fc581d371]
- Updated dependencies [00c12fa53]
- Updated dependencies [fc581d371]
  - @pnpm/resolve-dependencies@28.1.2
  - @pnpm/build-modules@9.2.2
  - dependency-path@9.2.2
  - @pnpm/headless@18.3.3
  - @pnpm/filter-lockfile@6.0.9
  - @pnpm/hoist@6.1.7
  - @pnpm/lockfile-to-pnp@0.5.14
  - @pnpm/lockfile-utils@4.0.8
  - @pnpm/lockfile-walker@5.0.8
  - @pnpm/modules-cleaner@12.0.11
  - @pnpm/package-requester@18.0.11
  - @pnpm/prune-lockfile@4.0.9

## 5.5.3

### Patch Changes

- 7922d6314: Don't link local dev dependencies, when prod dependencies should only be installed.
  - @pnpm/package-requester@18.0.10
  - @pnpm/headless@18.3.2

## 5.5.2

### Patch Changes

- 12aa1e2e1: Return early when the lockfile is up-to-date.
  - @pnpm/lockfile-to-pnp@0.5.13
  - @pnpm/headless@18.3.2

## 5.5.1

### Patch Changes

- 8e5b77ef6: Update the dependencies when a patch file is modified.
- 285ff09ba: Patch packages even when scripts are ignored.
- Updated dependencies [285ff09ba]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [285ff09ba]
- Updated dependencies [8e5b77ef6]
  - @pnpm/calc-dep-state@3.0.1
  - @pnpm/build-modules@9.2.1
  - @pnpm/headless@18.3.1
  - @pnpm/resolve-dependencies@28.1.1
  - @pnpm/types@8.4.0
  - @pnpm/filter-lockfile@6.0.8
  - @pnpm/hoist@6.1.6
  - @pnpm/lockfile-file@5.1.2
  - @pnpm/lockfile-utils@4.0.7
  - @pnpm/lockfile-walker@5.0.7
  - @pnpm/modules-cleaner@12.0.10
  - @pnpm/prune-lockfile@4.0.8
  - @pnpm/core-loggers@7.0.5
  - dependency-path@9.2.1
  - @pnpm/get-context@6.2.2
  - @pnpm/lifecycle@13.1.1
  - @pnpm/link-bins@7.1.5
  - @pnpm/lockfile-to-pnp@0.5.12
  - @pnpm/manifest-utils@3.0.6
  - @pnpm/modules-yaml@10.0.5
  - @pnpm/normalize-registries@3.0.5
  - @pnpm/package-requester@18.0.10
  - @pnpm/read-package-json@6.0.6
  - @pnpm/read-project-manifest@3.0.6
  - @pnpm/remove-bins@3.0.6
  - @pnpm/resolver-base@9.0.5
  - @pnpm/store-controller-types@14.0.1
  - @pnpm/symlink-dependency@5.0.5

## 5.5.0

### Minor Changes

- 2a34b21ce: Dependencies patching is possible via the `pnpm.patchedDependencies` field of the `package.json`.
  To patch a package, the package name, exact version, and the relative path to the patch file should be specified. For instance:

  ```json
  {
    "pnpm": {
      "patchedDependencies": {
        "eslint@1.0.0": "./patches/eslint@1.0.0.patch"
      }
    }
  }
  ```

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
- Updated dependencies [2a34b21ce]
- Updated dependencies [2a34b21ce]
- Updated dependencies [2a34b21ce]
  - @pnpm/headless@18.3.0
  - @pnpm/types@8.3.0
  - @pnpm/resolve-dependencies@28.1.0
  - @pnpm/lifecycle@13.1.0
  - dependency-path@9.2.0
  - @pnpm/calc-dep-state@3.0.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/build-modules@9.2.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/filter-lockfile@6.0.7
  - @pnpm/get-context@6.2.1
  - @pnpm/hoist@6.1.5
  - @pnpm/link-bins@7.1.4
  - @pnpm/lockfile-file@5.1.1
  - @pnpm/lockfile-to-pnp@0.5.11
  - @pnpm/lockfile-utils@4.0.6
  - @pnpm/lockfile-walker@5.0.6
  - @pnpm/manifest-utils@3.0.5
  - @pnpm/modules-cleaner@12.0.9
  - @pnpm/modules-yaml@10.0.4
  - @pnpm/normalize-registries@3.0.4
  - @pnpm/package-requester@18.0.9
  - @pnpm/prune-lockfile@4.0.7
  - @pnpm/read-package-json@6.0.5
  - @pnpm/read-project-manifest@3.0.5
  - @pnpm/remove-bins@3.0.5
  - @pnpm/resolver-base@9.0.4
  - @pnpm/symlink-dependency@5.0.4

## 5.4.0

### Minor Changes

- fb5bbfd7a: A new setting added: `pnpm.peerDependencyRules.allowAny`. `allowAny` is an array of package name patterns, any peer dependency matching the pattern will be resolved from any version, regardless of the range specified in `peerDependencies`. For instance:

  ```
  {
    "pnpm": {
      "peerDependencyRules": {
        "allowAny": ["@babel/*", "eslint"]
      }
    }
  }
  ```

  The above setting will mute any warnings about peer dependency version mismatches related to `@babel/` packages or `eslint`.

- fb5bbfd7a: The `pnpm.peerDependencyRules.ignoreMissing` setting may accept package name patterns. So you may ignore any missing `@babel/*` peer dependencies, for instance:

  ```json
  {
    "pnpm": {
      "peerDependencyRules": {
        "ignoreMissing": ["@babel/*"]
      }
    }
  }
  ```

- 56cf04cb3: New settings added: use-git-branch-lockfile, merge-git-branch-lockfiles, merge-git-branch-lockfiles-branch-pattern.

### Patch Changes

- 0abfe1718: Packages that should be built are always cloned or copied from the store. This is required to prevent the postinstall scripts from modifying the original source files of the package.
- Updated dependencies [fb5bbfd7a]
- Updated dependencies [0abfe1718]
- Updated dependencies [0abfe1718]
- Updated dependencies [0abfe1718]
- Updated dependencies [56cf04cb3]
- Updated dependencies [725636a90]
- Updated dependencies [0abfe1718]
  - @pnpm/types@8.2.0
  - @pnpm/build-modules@9.1.5
  - @pnpm/resolve-dependencies@28.0.0
  - @pnpm/headless@18.2.0
  - @pnpm/get-context@6.2.0
  - @pnpm/lockfile-file@5.1.0
  - dependency-path@9.1.4
  - @pnpm/package-requester@18.0.8
  - @pnpm/core-loggers@7.0.3
  - @pnpm/filter-lockfile@6.0.6
  - @pnpm/hoist@6.1.4
  - @pnpm/lifecycle@13.0.5
  - @pnpm/link-bins@7.1.3
  - @pnpm/lockfile-to-pnp@0.5.10
  - @pnpm/lockfile-utils@4.0.5
  - @pnpm/lockfile-walker@5.0.5
  - @pnpm/manifest-utils@3.0.4
  - @pnpm/modules-cleaner@12.0.8
  - @pnpm/modules-yaml@10.0.3
  - @pnpm/normalize-registries@3.0.3
  - @pnpm/prune-lockfile@4.0.6
  - @pnpm/read-package-json@6.0.4
  - @pnpm/read-project-manifest@3.0.4
  - @pnpm/remove-bins@3.0.4
  - @pnpm/resolver-base@9.0.3
  - @pnpm/store-controller-types@13.0.4
  - @pnpm/symlink-dependency@5.0.3

## 5.3.1

### Patch Changes

- @pnpm/lockfile-to-pnp@0.5.9
- @pnpm/headless@18.1.11

## 5.3.0

### Minor Changes

- 4d39e4a0c: A new setting is supported for ignoring specific deprecation messages: `pnpm.allowedDeprecatedVersions`. The setting should be provided in the `pnpm` section of the root `package.json` file. The below example will mute any deprecation warnings about the `request` package and warnings about `express` v1:

  ```json
  {
    "pnpm": {
      "allowedDeprecatedVersions": {
        "request": "*",
        "express": "1"
      }
    }
  }
  ```

  Related issue: [#4306](https://github.com/pnpm/pnpm/issues/4306)
  Related PR: [#4864](https://github.com/pnpm/pnpm/pull/4864)

### Patch Changes

- c1238946f: Update the compatibility database.
- Updated dependencies [4d39e4a0c]
- Updated dependencies [4d39e4a0c]
- Updated dependencies [26413c30c]
  - @pnpm/types@8.1.0
  - @pnpm/resolve-dependencies@27.2.0
  - @pnpm/build-modules@9.1.4
  - @pnpm/core-loggers@7.0.2
  - dependency-path@9.1.3
  - @pnpm/filter-lockfile@6.0.5
  - @pnpm/get-context@6.1.3
  - @pnpm/headless@18.1.10
  - @pnpm/hoist@6.1.3
  - @pnpm/lifecycle@13.0.4
  - @pnpm/link-bins@7.1.2
  - @pnpm/lockfile-file@5.0.4
  - @pnpm/lockfile-to-pnp@0.5.8
  - @pnpm/lockfile-utils@4.0.4
  - @pnpm/lockfile-walker@5.0.4
  - @pnpm/manifest-utils@3.0.3
  - @pnpm/modules-cleaner@12.0.7
  - @pnpm/modules-yaml@10.0.2
  - @pnpm/normalize-registries@3.0.2
  - @pnpm/package-requester@18.0.7
  - @pnpm/prune-lockfile@4.0.5
  - @pnpm/read-package-json@6.0.3
  - @pnpm/read-project-manifest@3.0.3
  - @pnpm/remove-bins@3.0.3
  - @pnpm/resolver-base@9.0.2
  - @pnpm/store-controller-types@13.0.3
  - @pnpm/symlink-dependency@5.0.2

## 5.2.5

### Patch Changes

- Updated dependencies [9f5352014]
  - @pnpm/resolve-dependencies@27.1.4

## 5.2.4

### Patch Changes

- 6756c2b02: It should be possible to install a git-hosted package that has no `package.json` file [#4822](https://github.com/pnpm/pnpm/issues/4822).
- Updated dependencies [6756c2b02]
  - @pnpm/build-modules@9.1.3
  - @pnpm/package-requester@18.0.6
  - @pnpm/resolve-dependencies@27.1.3
  - @pnpm/store-controller-types@13.0.2
  - @pnpm/headless@18.1.9
  - @pnpm/lifecycle@13.0.3
  - @pnpm/modules-cleaner@12.0.6
  - @pnpm/link-bins@7.1.1

## 5.2.3

### Patch Changes

- Updated dependencies [971f2c4a5]
- Updated dependencies [2b543c774]
  - @pnpm/build-modules@9.1.2
  - @pnpm/resolve-dependencies@27.1.2
  - @pnpm/headless@18.1.8

## 5.2.2

### Patch Changes

- Updated dependencies [45238e358]
  - @pnpm/resolve-dependencies@27.1.1
  - @pnpm/lockfile-to-pnp@0.5.7
  - @pnpm/headless@18.1.7

## 5.2.1

### Patch Changes

- @pnpm/lockfile-to-pnp@0.5.6
- @pnpm/headless@18.1.6

## 5.2.0

### Minor Changes

- 190f0b331: New option added for automatically installing missing peer dependencies: `autoInstallPeers`.

### Patch Changes

- Updated dependencies [190f0b331]
- Updated dependencies [190f0b331]
  - @pnpm/resolve-dependencies@27.1.0
  - @pnpm/prune-lockfile@4.0.4

## 5.1.2

### Patch Changes

- Updated dependencies [c57695550]
  - dependency-path@9.1.2
  - @pnpm/lockfile-to-pnp@0.5.5
  - @pnpm/filter-lockfile@6.0.4
  - @pnpm/headless@18.1.5
  - @pnpm/hoist@6.1.2
  - @pnpm/lockfile-utils@4.0.3
  - @pnpm/lockfile-walker@5.0.3
  - @pnpm/modules-cleaner@12.0.5
  - @pnpm/package-requester@18.0.5
  - @pnpm/prune-lockfile@4.0.3
  - @pnpm/resolve-dependencies@27.0.4

## 5.1.1

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/filter-lockfile@6.0.3
  - @pnpm/lockfile-file@5.0.3
  - @pnpm/resolve-dependencies@27.0.3
  - @pnpm/headless@18.1.4
  - @pnpm/modules-cleaner@12.0.4
  - @pnpm/get-context@6.1.2
  - @pnpm/lockfile-to-pnp@0.5.4
  - @pnpm/package-requester@18.0.4

## 5.1.0

### Minor Changes

- 0075fcd23: The `install()` function accepts the `pruneDirectDependencies` option.

### Patch Changes

- cadefe5b6: Print a warning when the integrity of more than 1K files is checked in the CAFS.
- 315871260: Use Yarn's compatibility database to patch broken packages in the ecosystem with package extensions.
- Updated dependencies [0075fcd23]
  - @pnpm/modules-cleaner@12.0.3
  - @pnpm/package-requester@18.0.3
  - @pnpm/headless@18.1.3
  - @pnpm/link-bins@7.1.1

## 5.0.0

### Major Changes

- af6ac00e4: Remove linkFromGlobal and linkToGlobal.

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/package-requester@18.0.2
  - @pnpm/build-modules@9.1.1
  - @pnpm/core-loggers@7.0.1
  - dependency-path@9.1.1
  - @pnpm/filter-lockfile@6.0.2
  - @pnpm/get-context@6.1.1
  - @pnpm/headless@18.1.2
  - @pnpm/hoist@6.1.1
  - @pnpm/lifecycle@13.0.2
  - @pnpm/link-bins@7.1.1
  - @pnpm/lockfile-file@5.0.2
  - @pnpm/lockfile-to-pnp@0.5.3
  - @pnpm/lockfile-utils@4.0.2
  - @pnpm/lockfile-walker@5.0.2
  - @pnpm/manifest-utils@3.0.2
  - @pnpm/modules-cleaner@12.0.2
  - @pnpm/modules-yaml@10.0.1
  - @pnpm/normalize-registries@3.0.1
  - @pnpm/prune-lockfile@4.0.2
  - @pnpm/read-package-json@6.0.2
  - @pnpm/read-project-manifest@3.0.2
  - @pnpm/remove-bins@3.0.2
  - @pnpm/resolve-dependencies@27.0.2
  - @pnpm/resolver-base@9.0.1
  - @pnpm/store-controller-types@13.0.1
  - @pnpm/symlink-dependency@5.0.1

## 4.0.2

### Patch Changes

- Updated dependencies [3345c2cce]
- Updated dependencies [7478cbd05]
  - @pnpm/resolve-dependencies@27.0.1

## 4.0.1

### Patch Changes

- c38feff08: Only `pnpm install` should fail on peer dependency issues.
  - @pnpm/lockfile-to-pnp@0.5.2
  - @pnpm/headless@18.1.1

## 4.0.0

### Major Changes

- 0a70aedb1: Use a base32 hash instead of a hex to encode too long dependency paths inside `node_modules/.pnpm` [#4552](https://github.com/pnpm/pnpm/pull/4552).
- e7bdc2cc2: Dependencies of the root workspace project are not used to resolve peer dependencies of other workspace projects [#4469](https://github.com/pnpm/pnpm/pull/4469).

### Patch Changes

- 2109f2e8e: Use `@pnpm/graph-sequencer` instead of `graph-sequencer`.
- 88289a42c: peerDependencyRules will no longer cause duplicated peer dependency rules in the lockfile when used in workspaces
- aecd4acdd: Linked in dependencies should be considered when resolving peer dependencies [#4541](https://github.com/pnpm/pnpm/pull/4541).
- dbe366990: Peer dependency should be correctly resolved from the workspace, when it is declared using a workspace protocol [#4529](https://github.com/pnpm/pnpm/issues/4529).
- Updated dependencies [948a8151e]
- Updated dependencies [0a70aedb1]
- Updated dependencies [8fa95fd86]
- Updated dependencies [2109f2e8e]
- Updated dependencies [8fa95fd86]
- Updated dependencies [0a70aedb1]
- Updated dependencies [e531325c3]
- Updated dependencies [7cdca5ef2]
- Updated dependencies [e7bdc2cc2]
- Updated dependencies [688b0eaff]
- Updated dependencies [aecd4acdd]
- Updated dependencies [dbe366990]
- Updated dependencies [b716d2d06]
- Updated dependencies [618842b0d]
- Updated dependencies [1267e4eff]
  - @pnpm/resolve-dependencies@27.0.0
  - dependency-path@9.1.0
  - @pnpm/build-modules@9.1.0
  - @pnpm/headless@18.1.0
  - @pnpm/hoist@6.1.0
  - @pnpm/link-bins@7.1.0
  - @pnpm/get-context@6.1.0
  - @pnpm/package-requester@18.0.1
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/manifest-utils@3.0.1
  - @pnpm/constants@6.1.0
  - @pnpm/filter-lockfile@6.0.1
  - @pnpm/lockfile-to-pnp@0.5.1
  - @pnpm/lockfile-walker@5.0.1
  - @pnpm/modules-cleaner@12.0.1
  - @pnpm/prune-lockfile@4.0.1
  - @pnpm/lifecycle@13.0.1
  - @pnpm/calc-dep-state@2.0.1
  - @pnpm/error@3.0.1
  - @pnpm/lockfile-file@5.0.1
  - @pnpm/parse-overrides@2.0.1
  - @pnpm/read-package-json@6.0.1
  - @pnpm/read-project-manifest@3.0.1
  - @pnpm/remove-bins@3.0.1

## 3.0.0

### Major Changes

- 516859178: `extendNodePath` removed.
- a36b6026b: pruneLockfileImporters is true by default.
- 73d71a2d5: `strict-peer-dependencies` is `true` by default.
- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [9c22c063e]
- Updated dependencies [faf830b8f]
- Updated dependencies [9b9b13c3a]
- Updated dependencies [542014839]
- Updated dependencies [0845a8704]
- Updated dependencies [d999a0801]
  - @pnpm/build-modules@9.0.0
  - @pnpm/headless@18.0.0
  - @pnpm/hoist@6.0.0
  - @pnpm/link-bins@7.0.0
  - @pnpm/types@8.0.0
  - @pnpm/package-requester@18.0.0
  - dependency-path@9.0.0
  - @pnpm/resolve-dependencies@26.0.0
  - @pnpm/calc-dep-state@2.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/filter-lockfile@6.0.0
  - @pnpm/get-context@6.0.0
  - @pnpm/lifecycle@13.0.0
  - @pnpm/lockfile-file@5.0.0
  - @pnpm/lockfile-utils@4.0.0
  - @pnpm/lockfile-walker@5.0.0
  - @pnpm/manifest-utils@3.0.0
  - @pnpm/modules-cleaner@12.0.0
  - @pnpm/modules-yaml@10.0.0
  - @pnpm/normalize-registries@3.0.0
  - @pnpm/parse-overrides@2.0.0
  - @pnpm/parse-wanted-dependency@3.0.0
  - @pnpm/prune-lockfile@4.0.0
  - @pnpm/read-modules-dir@4.0.0
  - @pnpm/read-package-json@6.0.0
  - @pnpm/read-project-manifest@3.0.0
  - @pnpm/remove-bins@3.0.0
  - @pnpm/resolver-base@9.0.0
  - @pnpm/store-controller-types@13.0.0
  - @pnpm/symlink-dependency@5.0.0
  - @pnpm/which-version-is-pinned@2.0.0
  - @pnpm/lockfile-to-pnp@0.5.0

## 2.7.3

### Patch Changes

- Updated dependencies [4941f31ee]
  - @pnpm/resolve-dependencies@25.0.2

## 2.7.2

### Patch Changes

- 5c525db13: In order to guarantee that only correct data is written to the store, data from the lockfile should not be written to the store. Only data directly from the package tarball or package metadata.
- Updated dependencies [5c525db13]
- Updated dependencies [70ba51da9]
- Updated dependencies [70ba51da9]
- Updated dependencies [5c525db13]
  - @pnpm/resolve-dependencies@25.0.1
  - @pnpm/filter-lockfile@5.0.19
  - @pnpm/error@2.1.0
  - @pnpm/package-requester@17.0.0
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/headless@17.3.2
  - @pnpm/modules-cleaner@11.0.23
  - @pnpm/get-context@5.3.8
  - @pnpm/link-bins@6.2.12
  - @pnpm/lockfile-file@4.3.1
  - @pnpm/manifest-utils@2.1.9
  - @pnpm/parse-overrides@1.0.1
  - @pnpm/read-package-json@5.0.12
  - @pnpm/read-project-manifest@2.0.13
  - @pnpm/build-modules@8.0.3
  - @pnpm/lifecycle@12.1.7
  - @pnpm/lockfile-to-pnp@0.4.47
  - @pnpm/hoist@5.2.15
  - @pnpm/remove-bins@2.0.14

## 2.7.1

### Patch Changes

- 4e3b99ae0: `onlyBuiltDependencies` should work.

## 2.7.0

### Minor Changes

- b138d048c: New optional field supported: `onlyBuiltDependencies`.
- d84b73b15: When adding a new dependency, use the version specifier from the overrides, when present [#4313](https://github.com/pnpm/pnpm/issues/4313).

  Normally, if the latest version of `foo` is `2.0.0`, then `pnpm add foo` installs `foo@^2.0.0`. This behavior changes if `foo` is specified in an override:

  ```json
  {
    "pnpm": {
      "overrides": {
        "foo": "1.0.0"
      }
    }
  }
  ```

  In this case, `pnpm add foo` will add `foo@1.0.0` to the dependency. However, if a version is explicitly specifying, then the specified version will be used and the override will be ignored. So `pnpm add foo@0` will install v0 and it doesn't matter what is in the overrides.

### Patch Changes

- 076c3753a: When a peer dependency range is extended with `*`, just replace any range with `*`.
- Updated dependencies [800fb2836]
- Updated dependencies [b138d048c]
- Updated dependencies [b138d048c]
  - @pnpm/package-requester@16.0.2
  - @pnpm/lockfile-file@4.3.0
  - @pnpm/types@7.10.0
  - @pnpm/resolve-dependencies@25.0.0
  - @pnpm/headless@17.3.1
  - @pnpm/get-context@5.3.7
  - @pnpm/lockfile-to-pnp@0.4.46
  - @pnpm/filter-lockfile@5.0.18
  - @pnpm/hoist@5.2.14
  - @pnpm/lockfile-utils@3.2.1
  - @pnpm/lockfile-walker@4.0.15
  - @pnpm/modules-cleaner@11.0.22
  - @pnpm/prune-lockfile@3.0.15
  - @pnpm/build-modules@8.0.2
  - @pnpm/core-loggers@6.1.4
  - dependency-path@8.0.11
  - @pnpm/lifecycle@12.1.6
  - @pnpm/link-bins@6.2.11
  - @pnpm/manifest-utils@2.1.8
  - @pnpm/modules-yaml@9.1.1
  - @pnpm/normalize-registries@2.0.13
  - @pnpm/read-package-json@5.0.11
  - @pnpm/read-project-manifest@2.0.12
  - @pnpm/remove-bins@2.0.13
  - @pnpm/resolver-base@8.1.6
  - @pnpm/store-controller-types@11.0.12
  - @pnpm/symlink-dependency@4.0.13

## 2.6.0

### Minor Changes

- 329e186e9: Allow to set hoistingLimits for the hoisted node linker.

### Patch Changes

- Updated dependencies [7ae349cd3]
- Updated dependencies [329e186e9]
  - @pnpm/lifecycle@12.1.5
  - @pnpm/headless@17.3.0
  - @pnpm/build-modules@8.0.1

## 2.5.4

### Patch Changes

- cc727797f: Add `publicHoistPattern` to the fields of `InstallOptions`.

## 2.5.3

### Patch Changes

- 37d09a68f: A package should be able to be a dependency of itself.
- Updated dependencies [37d09a68f]
- Updated dependencies [37d09a68f]
  - @pnpm/resolve-dependencies@24.0.0
  - @pnpm/headless@17.2.2
  - @pnpm/lockfile-to-pnp@0.4.45

## 2.5.2

### Patch Changes

- c1383044d: Fixed an exception that was caused by reading the name property from a manifest that was not defined.

## 2.5.1

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.44
- @pnpm/headless@17.2.1

## 2.5.0

### Minor Changes

- cdc521cfa: All the locations of injected dependencies are saved in the modules state file at `node_modules/.modules.yaml`.

### Patch Changes

- Updated dependencies [cdc521cfa]
- Updated dependencies [cdc521cfa]
- Updated dependencies [cdc521cfa]
  - @pnpm/headless@17.2.0
  - @pnpm/modules-yaml@9.1.0
  - @pnpm/lockfile-utils@3.2.0
  - @pnpm/get-context@5.3.6
  - @pnpm/filter-lockfile@5.0.17
  - @pnpm/hoist@5.2.13
  - @pnpm/lockfile-to-pnp@0.4.43
  - @pnpm/modules-cleaner@11.0.21
  - @pnpm/resolve-dependencies@23.0.4
  - @pnpm/link-bins@6.2.10
  - @pnpm/package-requester@16.0.1

## 2.4.1

### Patch Changes

- 08d781b80: `peerDependencyRules` should work when both `overrides` and `packageExtensions` are present as well.

## 2.4.0

### Minor Changes

- 1cadc231a: Side effects cache is not an experimental feature anymore.

  Side effects cache is saved separately for packages with different dependencies. So if `foo` has `bar` in the dependencies, then a separate cache will be created each time `foo` is installed with a different version of `bar` [#4238](https://github.com/pnpm/pnpm/pull/4238).

### Patch Changes

- 4bdf7bcac: `@pnpm/registry-mock` should be a dev dependency.
- Updated dependencies [43e4246d3]
- Updated dependencies [1cadc231a]
- Updated dependencies [8a2cad034]
- Updated dependencies [1cadc231a]
- Updated dependencies [1cadc231a]
  - @pnpm/headless@17.1.0
  - @pnpm/manifest-utils@2.1.7
  - @pnpm/calc-dep-state@1.0.0
  - @pnpm/build-modules@8.0.0
  - @pnpm/lockfile-to-pnp@0.4.42
  - @pnpm/link-bins@6.2.10
  - @pnpm/resolve-dependencies@23.0.3
  - @pnpm/hoist@5.2.12

## 2.3.0

### Minor Changes

- 26cd01b88: New optional option supported: `peerDependencyRules`. This setting allows to mute specific peer dependency warnings.
- e76151f66: `mutateModules()` returns the peer dependency issues of each installed project.

### Patch Changes

- 50ee25ae2: Export `MutateModulesOptions`.
- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/lifecycle@12.1.4
  - @pnpm/lockfile-to-pnp@0.4.41
  - @pnpm/build-modules@7.2.5
  - @pnpm/core-loggers@6.1.3
  - dependency-path@8.0.10
  - @pnpm/filter-lockfile@5.0.16
  - @pnpm/get-context@5.3.5
  - @pnpm/headless@17.0.3
  - @pnpm/hoist@5.2.11
  - @pnpm/link-bins@6.2.9
  - @pnpm/lockfile-file@4.2.6
  - @pnpm/lockfile-utils@3.1.6
  - @pnpm/lockfile-walker@4.0.14
  - @pnpm/manifest-utils@2.1.6
  - @pnpm/modules-cleaner@11.0.20
  - @pnpm/modules-yaml@9.0.11
  - @pnpm/normalize-registries@2.0.12
  - @pnpm/package-requester@16.0.1
  - @pnpm/prune-lockfile@3.0.14
  - @pnpm/read-package-json@5.0.10
  - @pnpm/read-project-manifest@2.0.11
  - @pnpm/remove-bins@2.0.12
  - @pnpm/resolve-dependencies@23.0.2
  - @pnpm/resolver-base@8.1.5
  - @pnpm/store-controller-types@11.0.11
  - @pnpm/symlink-dependency@4.0.12

## 2.2.6

### Patch Changes

- Updated dependencies [0b78577f5]
- Updated dependencies [ea24c69fe]
  - @pnpm/headless@17.0.2
  - @pnpm/build-modules@7.2.4

## 2.2.5

### Patch Changes

- Updated dependencies [df69150fc]
- Updated dependencies [cbd2f3e2a]
  - @pnpm/headless@17.0.1
  - @pnpm/resolve-dependencies@23.0.1

## 2.2.4

### Patch Changes

- Updated dependencies [8ddcd5116]
- Updated dependencies [8ddcd5116]
  - @pnpm/headless@17.0.0
  - @pnpm/resolve-dependencies@23.0.0
  - @pnpm/package-requester@16.0.0

## 2.2.3

### Patch Changes

- Updated dependencies [0b5662fc5]
  - @pnpm/headless@16.4.3

## 2.2.2

### Patch Changes

- 7bac7e8be: Don't write a lockfile if useLockfile is set to false.
- 7375396db: Save the value of the active `nodeLinker` to `node_modules/.modules.yaml`.
- Updated dependencies [7375396db]
  - @pnpm/headless@16.4.2
  - @pnpm/modules-yaml@9.0.10
  - @pnpm/lockfile-to-pnp@0.4.40
  - @pnpm/get-context@5.3.4
  - @pnpm/link-bins@6.2.8
  - @pnpm/package-requester@15.2.6

## 2.2.1

### Patch Changes

- @pnpm/headless@16.4.1

## 2.2.0

### Minor Changes

- 732d4962f: nodeLinker may accept two new values: `isolated` and `hoisted`.

  `hoisted` will create a "classic" `node_modules` folder without using symlinks.

  `isolated` will be the default value that creates a symlinked `node_modules`.

### Patch Changes

- Updated dependencies [732d4962f]
  - @pnpm/headless@16.4.0
  - @pnpm/package-requester@15.2.6
  - @pnpm/lockfile-to-pnp@0.4.39

## 2.1.4

### Patch Changes

- Updated dependencies [b5734a4a7]
- Updated dependencies [701ea0746]
- Updated dependencies [b390c75a6]
- Updated dependencies [b5734a4a7]
  - @pnpm/resolve-dependencies@22.1.0
  - @pnpm/link-bins@6.2.8
  - @pnpm/types@7.8.0
  - @pnpm/build-modules@7.2.3
  - @pnpm/headless@16.3.8
  - @pnpm/hoist@5.2.10
  - @pnpm/core-loggers@6.1.2
  - dependency-path@8.0.9
  - @pnpm/filter-lockfile@5.0.15
  - @pnpm/get-context@5.3.3
  - @pnpm/lifecycle@12.1.3
  - @pnpm/lockfile-file@4.2.5
  - @pnpm/lockfile-to-pnp@0.4.38
  - @pnpm/lockfile-utils@3.1.5
  - @pnpm/lockfile-walker@4.0.13
  - @pnpm/manifest-utils@2.1.5
  - @pnpm/modules-cleaner@11.0.19
  - @pnpm/modules-yaml@9.0.9
  - @pnpm/normalize-registries@2.0.11
  - @pnpm/package-requester@15.2.6
  - @pnpm/prune-lockfile@3.0.13
  - @pnpm/read-package-json@5.0.9
  - @pnpm/read-project-manifest@2.0.10
  - @pnpm/remove-bins@2.0.11
  - @pnpm/resolver-base@8.1.4
  - @pnpm/store-controller-types@11.0.10
  - @pnpm/symlink-dependency@4.0.11

## 2.1.3

### Patch Changes

- 08380076f: Add more details to the frozen lockfile error.
- Updated dependencies [08380076f]
- Updated dependencies [eb9ebd0f3]
- Updated dependencies [eb9ebd0f3]
  - @pnpm/headless@16.3.7
  - @pnpm/lockfile-file@4.2.4
  - @pnpm/get-context@5.3.2
  - @pnpm/lockfile-to-pnp@0.4.37

## 2.1.2

### Patch Changes

- cb2e4e33a: Export peer dependency issue types.
- Updated dependencies [7962c042e]
  - @pnpm/resolve-dependencies@22.0.2

## 2.1.1

### Patch Changes

- Updated dependencies [6493e0c93]
- Updated dependencies [cb1827b9c]
  - @pnpm/types@7.7.1
  - @pnpm/resolve-dependencies@22.0.1
  - @pnpm/build-modules@7.2.2
  - @pnpm/core-loggers@6.1.1
  - dependency-path@8.0.8
  - @pnpm/filter-lockfile@5.0.14
  - @pnpm/get-context@5.3.1
  - @pnpm/headless@16.3.6
  - @pnpm/hoist@5.2.9
  - @pnpm/lifecycle@12.1.2
  - @pnpm/link-bins@6.2.7
  - @pnpm/lockfile-file@4.2.3
  - @pnpm/lockfile-to-pnp@0.4.36
  - @pnpm/lockfile-utils@3.1.4
  - @pnpm/lockfile-walker@4.0.12
  - @pnpm/manifest-utils@2.1.4
  - @pnpm/modules-cleaner@11.0.18
  - @pnpm/modules-yaml@9.0.8
  - @pnpm/normalize-registries@2.0.10
  - @pnpm/package-requester@15.2.5
  - @pnpm/prune-lockfile@3.0.12
  - @pnpm/read-package-json@5.0.8
  - @pnpm/read-project-manifest@2.0.9
  - @pnpm/remove-bins@2.0.10
  - @pnpm/resolver-base@8.1.3
  - @pnpm/store-controller-types@11.0.9
  - @pnpm/symlink-dependency@4.0.10

## 2.1.0

### Minor Changes

- 25f0fa9fa: New function added to the core API: `getPeerDependencyIssues()`.

### Patch Changes

- 5af305f39: Installation should be finished before an error about bad/missing peer dependencies is printed and kills the process.
- Updated dependencies [ae32d313e]
- Updated dependencies [ba9b2eba1]
- Updated dependencies [77ff0898b]
- Updated dependencies [25f0fa9fa]
- Updated dependencies [30bfca967]
- Updated dependencies [5af305f39]
- Updated dependencies [ae32d313e]
- Updated dependencies [ba9b2eba1]
- Updated dependencies [25f0fa9fa]
- Updated dependencies [a626c60fc]
  - @pnpm/which-version-is-pinned@1.0.0
  - @pnpm/core-loggers@6.1.0
  - @pnpm/package-requester@15.2.4
  - @pnpm/resolve-dependencies@22.0.0
  - @pnpm/normalize-registries@2.0.9
  - @pnpm/types@7.7.0
  - @pnpm/get-context@5.3.0
  - @pnpm/build-modules@7.2.1
  - @pnpm/headless@16.3.5
  - @pnpm/lifecycle@12.1.1
  - @pnpm/manifest-utils@2.1.3
  - @pnpm/modules-cleaner@11.0.17
  - @pnpm/remove-bins@2.0.9
  - @pnpm/symlink-dependency@4.0.9
  - @pnpm/lockfile-to-pnp@0.4.35
  - dependency-path@8.0.7
  - @pnpm/filter-lockfile@5.0.13
  - @pnpm/hoist@5.2.8
  - @pnpm/link-bins@6.2.6
  - @pnpm/lockfile-file@4.2.2
  - @pnpm/lockfile-utils@3.1.3
  - @pnpm/lockfile-walker@4.0.11
  - @pnpm/modules-yaml@9.0.7
  - @pnpm/prune-lockfile@3.0.11
  - @pnpm/read-package-json@5.0.7
  - @pnpm/read-project-manifest@2.0.8
  - @pnpm/resolver-base@8.1.2
  - @pnpm/store-controller-types@11.0.8

## 2.0.1

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/normalize-registries@2.0.8
  - @pnpm/lockfile-to-pnp@0.4.34
  - @pnpm/headless@16.3.4
  - @pnpm/package-requester@15.2.3
  - @pnpm/get-context@5.2.2

## 2.0.0

### Major Changes

- 8a99a01ff: `packageExtensions`, `overrides`, and `neverBuiltDependencies` are passed through as options to the core API. These settings are not read from the root manifest's `package.json`.

### Patch Changes

- Updated dependencies [3cf543fc1]
  - @pnpm/lockfile-utils@3.1.2
  - @pnpm/resolve-dependencies@21.2.3
  - @pnpm/filter-lockfile@5.0.12
  - @pnpm/headless@16.3.3
  - @pnpm/hoist@5.2.7
  - @pnpm/lockfile-to-pnp@0.4.33
  - @pnpm/modules-cleaner@11.0.16

## 1.3.2

### Patch Changes

- Updated dependencies [a7ff2d5ce]
- Updated dependencies [dbd8acfe9]
- Updated dependencies [119b3a908]
  - @pnpm/normalize-registries@2.0.7
  - @pnpm/package-requester@15.2.3
  - @pnpm/lockfile-to-pnp@0.4.32
  - @pnpm/headless@16.3.2
  - @pnpm/get-context@5.2.1

## 1.3.1

### Patch Changes

- fe9818220: The `registries` object should be read from the context not the options.
- Updated dependencies [b7fbd8c33]
  - @pnpm/headless@16.3.1

## 1.3.0

### Minor Changes

- 002778559: New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/build-modules@7.2.0
  - @pnpm/headless@16.3.0
  - @pnpm/lifecycle@12.1.0
  - @pnpm/lockfile-to-pnp@0.4.31
  - @pnpm/package-requester@15.2.2

## 1.2.3

### Patch Changes

- @pnpm/resolve-dependencies@21.2.2
- @pnpm/headless@16.2.4
- @pnpm/package-requester@15.2.2

## 1.2.2

### Patch Changes

- Updated dependencies [828e3b9e4]
- Updated dependencies [631877ebf]
  - @pnpm/resolve-dependencies@21.2.1
  - @pnpm/symlink-dependency@4.0.8
  - @pnpm/headless@16.2.4
  - @pnpm/hoist@5.2.6
  - @pnpm/package-requester@15.2.2

## 1.2.1

### Patch Changes

- bb0f8bc16: Don't crash if a bin file cannot be created because the source files could not be found.
- Updated dependencies [bb0f8bc16]
  - @pnpm/link-bins@6.2.5
  - @pnpm/build-modules@7.1.7
  - @pnpm/headless@16.2.3
  - @pnpm/hoist@5.2.5
  - @pnpm/filter-lockfile@5.0.11
  - @pnpm/package-requester@15.2.2
  - @pnpm/modules-cleaner@11.0.15

## 1.2.0

### Minor Changes

- 302ae4f6f: Support async hooks
- 2511c82cd: Added support for a new lifecycle script: `pnpm:devPreinstall`. This script works only in the root `package.json` file, only during local development, and runs before installation happens.

### Patch Changes

- Updated dependencies [302ae4f6f]
- Updated dependencies [fa03cbdc8]
- Updated dependencies [108bd4a39]
  - @pnpm/get-context@5.2.0
  - @pnpm/resolve-dependencies@21.2.0
  - @pnpm/types@7.6.0
  - @pnpm/lifecycle@12.0.2
  - @pnpm/build-modules@7.1.6
  - @pnpm/core-loggers@6.0.6
  - dependency-path@8.0.6
  - @pnpm/filter-lockfile@5.0.10
  - @pnpm/headless@16.2.2
  - @pnpm/hoist@5.2.4
  - @pnpm/link-bins@6.2.4
  - @pnpm/lockfile-file@4.2.1
  - @pnpm/lockfile-to-pnp@0.4.30
  - @pnpm/lockfile-utils@3.1.1
  - @pnpm/lockfile-walker@4.0.10
  - @pnpm/manifest-utils@2.1.2
  - @pnpm/modules-cleaner@11.0.14
  - @pnpm/modules-yaml@9.0.6
  - @pnpm/normalize-registries@2.0.6
  - @pnpm/package-requester@15.2.1
  - @pnpm/prune-lockfile@3.0.10
  - @pnpm/read-package-json@5.0.6
  - @pnpm/read-project-manifest@2.0.7
  - @pnpm/remove-bins@2.0.8
  - @pnpm/resolver-base@8.1.1
  - @pnpm/store-controller-types@11.0.7
  - @pnpm/symlink-dependency@4.0.7

## 1.1.2

### Patch Changes

- Updated dependencies [5b90ab98f]
  - @pnpm/lifecycle@12.0.1
  - @pnpm/build-modules@7.1.5
  - @pnpm/headless@16.2.1

## 1.1.1

### Patch Changes

- Updated dependencies [bc1c2aa62]
  - @pnpm/resolve-dependencies@21.1.1

## 1.1.0

### Minor Changes

- 4ab87844a: New property supported via the `dependenciesMeta` field of `package.json`: `injected`. When `injected` is set to `true`, the package will be hard linked to `node_modules`, not symlinked [#3915](https://github.com/pnpm/pnpm/pull/3915).

  For instance, the following `package.json` in a workspace will create a symlink to `bar` in the `node_modules` directory of `foo`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "workspace:1.0.0"
    }
  }
  ```

  But what if `bar` has `react` in its peer dependencies? If all projects in the monorepo use the same version of `react`, then no problem. But what if `bar` is required by `foo` that uses `react` 16 and `qar` with `react` 17? In the past, you'd have to choose a single version of react and install it as dev dependency of `bar`. But now with the `injected` field you can inject `bar` to a package, and `bar` will be installed with the `react` version of that package.

  So this will be the `package.json` of `foo`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "workspace:1.0.0",
      "react": "16"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar` will be hard linked into the dependencies of `foo`, and `react` 16 will be linked to the dependencies of `foo/node_modules/bar`.

  And this will be the `package.json` of `qar`:

  ```json
  {
    "name": "qar",
    "dependencies": {
      "bar": "workspace:1.0.0",
      "react": "17"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar` will be hard linked into the dependencies of `qar`, and `react` 17 will be linked to the dependencies of `qar/node_modules/bar`.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [37dcfceeb]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lifecycle@12.0.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/package-requester@15.2.0
  - @pnpm/resolve-dependencies@21.1.0
  - @pnpm/lockfile-file@4.2.0
  - @pnpm/lockfile-utils@3.1.0
  - @pnpm/headless@16.2.0
  - @pnpm/build-modules@7.1.4
  - @pnpm/core-loggers@6.0.5
  - dependency-path@8.0.5
  - @pnpm/filter-lockfile@5.0.9
  - @pnpm/get-context@5.1.6
  - @pnpm/hoist@5.2.3
  - @pnpm/link-bins@6.2.3
  - @pnpm/lockfile-to-pnp@0.4.29
  - @pnpm/lockfile-walker@4.0.9
  - @pnpm/manifest-utils@2.1.1
  - @pnpm/modules-cleaner@11.0.13
  - @pnpm/modules-yaml@9.0.5
  - @pnpm/normalize-registries@2.0.5
  - @pnpm/prune-lockfile@3.0.9
  - @pnpm/read-package-json@5.0.5
  - @pnpm/read-project-manifest@2.0.6
  - @pnpm/remove-bins@2.0.7
  - @pnpm/store-controller-types@11.0.6
  - @pnpm/symlink-dependency@4.0.6

## 1.0.2

### Patch Changes

- Updated dependencies [a916accec]
  - @pnpm/link-bins@6.2.2
  - @pnpm/build-modules@7.1.3
  - @pnpm/headless@16.1.6
  - @pnpm/hoist@5.2.2
  - @pnpm/package-requester@15.1.2

## 1.0.1

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.28
- @pnpm/resolve-dependencies@21.0.7
- @pnpm/headless@16.1.5
- @pnpm/package-requester@15.1.2

## 1.0.0

### Major Changes

- d4e2e52c4: Rename package from `supi` to `@pnpm/core`.

### Patch Changes

- Updated dependencies [6375cdce0]
  - @pnpm/link-bins@6.2.1
  - @pnpm/build-modules@7.1.2
  - @pnpm/headless@16.1.4
  - @pnpm/hoist@5.2.1
  - @pnpm/lockfile-to-pnp@0.4.27
  - @pnpm/package-requester@15.1.2

## 0.47.27

### Patch Changes

- Updated dependencies [4b163f69c]
  - @pnpm/resolve-dependencies@21.0.6
  - @pnpm/lockfile-to-pnp@0.4.26
  - @pnpm/headless@16.1.3

## 0.47.26

### Patch Changes

- e56cfaac8: `path-exists` should be a prod dependency.

## 0.47.25

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.25
- @pnpm/headless@16.1.2

## 0.47.24

### Patch Changes

- 59a4152ce: fix that hoisting all packages in the dependencies tree when using filtering
- Updated dependencies [4a4d42d8f]
- Updated dependencies [59a4152ce]
- Updated dependencies [59a4152ce]
  - @pnpm/lifecycle@11.0.5
  - @pnpm/hoist@5.2.0
  - @pnpm/headless@16.1.1
  - @pnpm/build-modules@7.1.1
  - @pnpm/package-requester@15.1.2

## 0.47.23

### Patch Changes

- c7081cbb4: New option added: `extendNodePath`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims.
- Updated dependencies [0d4a7c69e]
- Updated dependencies [c7081cbb4]
  - @pnpm/link-bins@6.2.0
  - @pnpm/remove-bins@2.0.6
  - @pnpm/build-modules@7.1.0
  - @pnpm/headless@16.1.0
  - @pnpm/hoist@5.1.0
  - @pnpm/modules-cleaner@11.0.12
  - @pnpm/lockfile-to-pnp@0.4.24

## 0.47.22

### Patch Changes

- 83e23601e: Do not override the bins of direct dependencies with the bins of hoisted dependencies.
- 6cc1aa2c0: `--fix-lockfile` should preserve existing lockfile's `dependencies` and `optionalDependencies`.
- Updated dependencies [83e23601e]
- Updated dependencies [553a5d840]
- Updated dependencies [83e23601e]
- Updated dependencies [553a5d840]
- Updated dependencies [b7e6f4428]
  - @pnpm/headless@16.0.29
  - @pnpm/manifest-utils@2.1.0
  - @pnpm/link-bins@6.1.0
  - @pnpm/resolve-dependencies@21.0.5
  - @pnpm/build-modules@7.0.10
  - @pnpm/hoist@5.0.14
  - @pnpm/lockfile-to-pnp@0.4.23

## 0.47.21

### Patch Changes

- 141d2f02e: Scripts should always be ignored when only the lockfile is being updated.
  - @pnpm/headless@16.0.28
  - @pnpm/package-requester@15.1.2

## 0.47.20

### Patch Changes

- 11a934da1: Adding --fix-lockfile for the install command to support autofix broken lockfile
- Updated dependencies [11a934da1]
- Updated dependencies [11a934da1]
  - @pnpm/package-requester@15.1.2
  - @pnpm/resolve-dependencies@21.0.4
  - @pnpm/lockfile-to-pnp@0.4.22
  - @pnpm/headless@16.0.28

## 0.47.19

### Patch Changes

- @pnpm/link-bins@6.0.8
- @pnpm/remove-bins@2.0.5
- @pnpm/resolve-dependencies@21.0.3
- @pnpm/headless@16.0.27
- @pnpm/package-requester@15.1.1
- @pnpm/build-modules@7.0.9
- @pnpm/hoist@5.0.13
- @pnpm/modules-cleaner@11.0.11

## 0.47.18

### Patch Changes

- ccf2f295d: Fix overrides that specify the parent package with a range.

## 0.47.17

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.21
- @pnpm/headless@16.0.26

## 0.47.16

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.20
- @pnpm/headless@16.0.25

## 0.47.15

### Patch Changes

- Updated dependencies [ee589ab9b]
  - @pnpm/resolve-dependencies@21.0.2
  - @pnpm/headless@16.0.24
  - @pnpm/package-requester@15.1.1

## 0.47.14

### Patch Changes

- Updated dependencies [31e01d9a9]
- Updated dependencies [31e01d9a9]
  - @pnpm/package-requester@15.1.1
  - @pnpm/resolve-dependencies@21.0.1
  - @pnpm/lockfile-to-pnp@0.4.19
  - @pnpm/headless@16.0.24

## 0.47.13

### Patch Changes

- Updated dependencies [07e7b1c0c]
- Updated dependencies [07e7b1c0c]
  - @pnpm/resolve-dependencies@21.0.0
  - @pnpm/package-requester@15.1.0
  - @pnpm/headless@16.0.23

## 0.47.12

### Patch Changes

- Updated dependencies [6208e2a71]
  - @pnpm/build-modules@7.0.8
  - @pnpm/resolve-dependencies@20.0.16
  - @pnpm/headless@16.0.22
  - @pnpm/package-requester@15.0.7

## 0.47.11

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.18
- @pnpm/headless@16.0.21

## 0.47.10

### Patch Changes

- Updated dependencies [135d53827]
  - @pnpm/resolve-dependencies@20.0.15
  - @pnpm/lockfile-to-pnp@0.4.17
  - @pnpm/headless@16.0.20

## 0.47.9

### Patch Changes

- Updated dependencies [71aab049d]
  - @pnpm/read-modules-dir@3.0.1
  - @pnpm/link-bins@6.0.7
  - @pnpm/modules-cleaner@11.0.10
  - @pnpm/lockfile-to-pnp@0.4.16
  - @pnpm/build-modules@7.0.7
  - @pnpm/headless@16.0.19
  - @pnpm/hoist@5.0.12

## 0.47.8

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/build-modules@7.0.6
  - @pnpm/core-loggers@6.0.4
  - dependency-path@8.0.4
  - @pnpm/filter-lockfile@5.0.8
  - @pnpm/get-context@5.1.5
  - @pnpm/headless@16.0.18
  - @pnpm/hoist@5.0.11
  - @pnpm/lifecycle@11.0.4
  - @pnpm/link-bins@6.0.6
  - @pnpm/lockfile-file@4.1.1
  - @pnpm/lockfile-to-pnp@0.4.15
  - @pnpm/lockfile-utils@3.0.8
  - @pnpm/lockfile-walker@4.0.8
  - @pnpm/manifest-utils@2.0.4
  - @pnpm/modules-cleaner@11.0.9
  - @pnpm/modules-yaml@9.0.4
  - @pnpm/normalize-registries@2.0.4
  - @pnpm/package-requester@15.0.7
  - @pnpm/prune-lockfile@3.0.8
  - @pnpm/read-package-json@5.0.4
  - @pnpm/read-project-manifest@2.0.5
  - @pnpm/remove-bins@2.0.4
  - @pnpm/resolve-dependencies@20.0.14
  - @pnpm/resolver-base@8.0.4
  - @pnpm/store-controller-types@11.0.5
  - @pnpm/symlink-dependency@4.0.5

## 0.47.7

### Patch Changes

- Updated dependencies [7af16a011]
  - @pnpm/lifecycle@11.0.3
  - @pnpm/build-modules@7.0.5
  - @pnpm/headless@16.0.17
  - @pnpm/lockfile-to-pnp@0.4.14

## 0.47.6

### Patch Changes

- 3c044519e: When adding new dependency to a workspace, prefer versions that are already installed in the workspace.

## 0.47.5

### Patch Changes

- 040124530: When adding a new dependency to a workspace, and the dependency is already present in the workspace (in another project), use the already present spec.

## 0.47.4

### Patch Changes

- ca67f6004: Override packages, when the parent package is set but no version range.

## 0.47.3

### Patch Changes

- caf453dd3: Overriding should work, when the range selector contains the ">" symbol.

## 0.47.2

### Patch Changes

- d3ec941d2: Linking should not fail on context checks.

## 0.47.1

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.13
- @pnpm/headless@16.0.16

## 0.47.0

### Minor Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- b3478c756: Never do full resolution when package manifest is ignored
  - @pnpm/lockfile-to-pnp@0.4.12
  - @pnpm/headless@16.0.15
  - @pnpm/package-requester@15.0.6
  - @pnpm/resolve-dependencies@20.0.13

## 0.46.18

### Patch Changes

- Updated dependencies [389858509]
  - @pnpm/resolve-dependencies@20.0.12

## 0.46.17

### Patch Changes

- 8e76690f4: A new optional field supported in the root `package.json` file: `pnpm.packageExtensions`. This new field allows to extend manifests of dependencies during installation.
- Updated dependencies [8e76690f4]
- Updated dependencies [8e76690f4]
  - @pnpm/lockfile-file@4.1.0
  - @pnpm/types@7.3.0
  - @pnpm/get-context@5.1.4
  - @pnpm/headless@16.0.14
  - @pnpm/lockfile-to-pnp@0.4.11
  - @pnpm/build-modules@7.0.4
  - @pnpm/core-loggers@6.0.3
  - dependency-path@8.0.3
  - @pnpm/filter-lockfile@5.0.7
  - @pnpm/hoist@5.0.10
  - @pnpm/lifecycle@11.0.2
  - @pnpm/link-bins@6.0.5
  - @pnpm/lockfile-utils@3.0.7
  - @pnpm/lockfile-walker@4.0.7
  - @pnpm/manifest-utils@2.0.3
  - @pnpm/modules-cleaner@11.0.8
  - @pnpm/modules-yaml@9.0.3
  - @pnpm/normalize-registries@2.0.3
  - @pnpm/package-requester@15.0.6
  - @pnpm/prune-lockfile@3.0.7
  - @pnpm/read-package-json@5.0.3
  - @pnpm/read-project-manifest@2.0.4
  - @pnpm/remove-bins@2.0.3
  - @pnpm/resolve-dependencies@20.0.11
  - @pnpm/resolver-base@8.0.3
  - @pnpm/store-controller-types@11.0.4
  - @pnpm/symlink-dependency@4.0.4

## 0.46.16

### Patch Changes

- Updated dependencies [6c418943c]
- Updated dependencies [c1cdc0184]
- Updated dependencies [060c73677]
  - dependency-path@8.0.2
  - @pnpm/resolve-dependencies@20.0.10
  - @pnpm/filter-lockfile@5.0.6
  - @pnpm/headless@16.0.13
  - @pnpm/hoist@5.0.9
  - @pnpm/lockfile-to-pnp@0.4.10
  - @pnpm/lockfile-utils@3.0.6
  - @pnpm/lockfile-walker@4.0.6
  - @pnpm/modules-cleaner@11.0.7
  - @pnpm/package-requester@15.0.5
  - @pnpm/prune-lockfile@3.0.6

## 0.46.15

### Patch Changes

- Updated dependencies [2dc5a7a4c]
  - @pnpm/lockfile-file@4.0.4
  - @pnpm/get-context@5.1.3
  - @pnpm/headless@16.0.12
  - @pnpm/lockfile-to-pnp@0.4.9

## 0.46.14

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/headless@16.0.11
  - @pnpm/package-requester@15.0.4
  - @pnpm/build-modules@7.0.3
  - @pnpm/core-loggers@6.0.2
  - dependency-path@8.0.1
  - @pnpm/filter-lockfile@5.0.5
  - @pnpm/get-context@5.1.2
  - @pnpm/hoist@5.0.8
  - @pnpm/lifecycle@11.0.1
  - @pnpm/link-bins@6.0.4
  - @pnpm/lockfile-file@4.0.3
  - @pnpm/lockfile-to-pnp@0.4.8
  - @pnpm/lockfile-utils@3.0.5
  - @pnpm/lockfile-walker@4.0.5
  - @pnpm/manifest-utils@2.0.2
  - @pnpm/modules-cleaner@11.0.6
  - @pnpm/modules-yaml@9.0.2
  - @pnpm/normalize-registries@2.0.2
  - @pnpm/prune-lockfile@3.0.5
  - @pnpm/read-package-json@5.0.2
  - @pnpm/read-project-manifest@2.0.3
  - @pnpm/remove-bins@2.0.2
  - @pnpm/resolve-dependencies@20.0.9
  - @pnpm/resolver-base@8.0.2
  - @pnpm/store-controller-types@11.0.3
  - @pnpm/symlink-dependency@4.0.3

## 0.46.13

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/build-modules@7.0.2
  - @pnpm/filter-lockfile@5.0.4
  - @pnpm/get-context@5.1.1
  - @pnpm/headless@16.0.10
  - @pnpm/hoist@5.0.7
  - @pnpm/link-bins@6.0.3
  - @pnpm/lockfile-file@4.0.2
  - @pnpm/lockfile-to-pnp@0.4.7
  - @pnpm/lockfile-utils@3.0.4
  - @pnpm/lockfile-walker@4.0.4
  - @pnpm/modules-cleaner@11.0.5
  - @pnpm/package-requester@15.0.3
  - @pnpm/prune-lockfile@3.0.4
  - @pnpm/resolve-dependencies@20.0.8

## 0.46.12

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.6
- @pnpm/headless@16.0.9

## 0.46.11

### Patch Changes

- da0d4091d: The second argument to readPackage hook should always be the context object.
- Updated dependencies [0560ca63f]
  - @pnpm/hoist@5.0.6
  - @pnpm/headless@16.0.8

## 0.46.10

### Patch Changes

- 0e69ad440: Prefer headless install, when the lockfile is up-to-date and some packages are linked using relative path via `workspace:<path>`.

## 0.46.9

### Patch Changes

- Updated dependencies [20e2f235d]
- Updated dependencies [ec097f4ed]
  - dependency-path@8.0.0
  - @pnpm/hoist@5.0.5
  - @pnpm/filter-lockfile@5.0.3
  - @pnpm/headless@16.0.7
  - @pnpm/lockfile-to-pnp@0.4.5
  - @pnpm/lockfile-utils@3.0.3
  - @pnpm/lockfile-walker@4.0.3
  - @pnpm/modules-cleaner@11.0.4
  - @pnpm/package-requester@15.0.2
  - @pnpm/prune-lockfile@3.0.3
  - @pnpm/resolve-dependencies@20.0.7

## 0.46.8

### Patch Changes

- @pnpm/package-requester@15.0.1
- @pnpm/read-project-manifest@2.0.2
- @pnpm/resolve-dependencies@20.0.6
- @pnpm/headless@16.0.6
- @pnpm/link-bins@6.0.2
- @pnpm/lockfile-to-pnp@0.4.4
- @pnpm/build-modules@7.0.1
- @pnpm/hoist@5.0.4

## 0.46.7

### Patch Changes

- 66dbd06e6: Pass `childConcurrency` option to `@pnpm/headless`. Setting `childConcurrency` should have an effect during frozen lockfile installation.
  - @pnpm/headless@16.0.5
  - @pnpm/package-requester@15.0.0

## 0.46.6

### Patch Changes

- 3e3c3ff71: `preinstall` scripts should run after installing the dependencies (this is how it works with npm).
- Updated dependencies [3e3c3ff71]
- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
  - @pnpm/headless@16.0.5
  - @pnpm/package-requester@15.0.0
  - @pnpm/build-modules@7.0.0
  - @pnpm/lifecycle@11.0.0
  - @pnpm/store-controller-types@11.0.2
  - @pnpm/modules-cleaner@11.0.3
  - @pnpm/resolve-dependencies@20.0.5

## 0.46.5

### Patch Changes

- Updated dependencies [787b69908]
  - @pnpm/resolve-dependencies@20.0.4

## 0.46.4

### Patch Changes

- 97c64bae4: It should be possible to override dependencies with links.
- Updated dependencies [97c64bae4]
- Updated dependencies [6e9c112af]
- Updated dependencies [97c64bae4]
- Updated dependencies [1a9b4f812]
  - @pnpm/get-context@5.1.0
  - @pnpm/read-project-manifest@2.0.1
  - @pnpm/types@7.1.0
  - @pnpm/build-modules@6.0.1
  - @pnpm/lockfile-to-pnp@0.4.3
  - @pnpm/headless@16.0.4
  - @pnpm/link-bins@6.0.1
  - @pnpm/resolve-dependencies@20.0.3
  - @pnpm/package-requester@14.0.3
  - @pnpm/core-loggers@6.0.1
  - dependency-path@7.0.1
  - @pnpm/filter-lockfile@5.0.2
  - @pnpm/hoist@5.0.3
  - @pnpm/lifecycle@10.0.1
  - @pnpm/lockfile-file@4.0.1
  - @pnpm/lockfile-utils@3.0.2
  - @pnpm/lockfile-walker@4.0.2
  - @pnpm/manifest-utils@2.0.1
  - @pnpm/modules-cleaner@11.0.2
  - @pnpm/modules-yaml@9.0.1
  - @pnpm/normalize-registries@2.0.1
  - @pnpm/prune-lockfile@3.0.2
  - @pnpm/read-package-json@5.0.1
  - @pnpm/remove-bins@2.0.1
  - @pnpm/resolver-base@8.0.1
  - @pnpm/store-controller-types@11.0.1
  - @pnpm/symlink-dependency@4.0.2

## 0.46.3

### Patch Changes

- @pnpm/lockfile-to-pnp@0.4.2
- @pnpm/headless@16.0.3

## 0.46.2

### Patch Changes

- c70c77f89: Overrides should override devDependencies as well.
- Updated dependencies [6f198457d]
- Updated dependencies [cbc1a827c]
  - @pnpm/package-requester@14.0.2
  - @pnpm/symlink-dependency@4.0.1
  - @pnpm/headless@16.0.2
  - @pnpm/resolve-dependencies@20.0.2
  - @pnpm/hoist@5.0.2

## 0.46.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0
  - @pnpm/filter-lockfile@5.0.1
  - @pnpm/headless@16.0.1
  - @pnpm/hoist@5.0.1
  - @pnpm/lockfile-to-pnp@0.4.1
  - @pnpm/lockfile-utils@3.0.1
  - @pnpm/lockfile-walker@4.0.1
  - @pnpm/modules-cleaner@11.0.1
  - @pnpm/package-requester@14.0.1
  - @pnpm/prune-lockfile@3.0.1
  - @pnpm/resolve-dependencies@20.0.1

## 0.46.0

### Minor Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- 78470a32d: New option added: `modulesCacheMaxAge`. The default value of the setting is 10080 (7 days in seconds). `modulesCacheMaxAge` is the time in minutes after which pnpm should remove the orphan packages from node_modules.
- f2d3b6c8b: Overrides match dependencies by checking if the target range is a subset of the specified range, instead of making an exact match.
- 048c94871: `.pnp.js` renamed to `.pnp.cjs` in order to force CommonJS.
- 735d2ac79: support fetch package without package manifest
- 9e30b9659: Do not execute prepublish during installation.

### Patch Changes

- 945dc9f56: `pnpm.overrides` should work on direct dependencies as well.
- Updated dependencies [6871d74b2]
- Updated dependencies [06c6c9959]
- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [90487a3a8]
- Updated dependencies [155e70597]
- Updated dependencies [78470a32d]
- Updated dependencies [9c2a878c3]
- Updated dependencies [048c94871]
- Updated dependencies [e4efddbd2]
- Updated dependencies [8b66f26dc]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f7750baed]
- Updated dependencies [83645c8ed]
- Updated dependencies [7adc6e875]
- Updated dependencies [78470a32d]
- Updated dependencies [78470a32d]
- Updated dependencies [735d2ac79]
- Updated dependencies [9c2a878c3]
- Updated dependencies [78470a32d]
  - @pnpm/constants@5.0.0
  - @pnpm/link-bins@6.0.0
  - @pnpm/build-modules@6.0.0
  - @pnpm/core-loggers@6.0.0
  - dependency-path@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/filter-lockfile@5.0.0
  - @pnpm/get-context@5.0.0
  - @pnpm/headless@16.0.0
  - @pnpm/hoist@5.0.0
  - @pnpm/lifecycle@10.0.0
  - @pnpm/lockfile-file@4.0.0
  - @pnpm/lockfile-to-pnp@0.4.0
  - @pnpm/lockfile-utils@3.0.0
  - @pnpm/lockfile-walker@4.0.0
  - @pnpm/manifest-utils@2.0.0
  - @pnpm/modules-cleaner@11.0.0
  - @pnpm/modules-yaml@9.0.0
  - @pnpm/normalize-registries@2.0.0
  - @pnpm/package-requester@14.0.0
  - @pnpm/parse-wanted-dependency@2.0.0
  - @pnpm/prune-lockfile@3.0.0
  - @pnpm/read-modules-dir@3.0.0
  - @pnpm/read-package-json@5.0.0
  - @pnpm/read-project-manifest@2.0.0
  - @pnpm/remove-bins@2.0.0
  - @pnpm/resolve-dependencies@20.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/store-controller-types@11.0.0
  - @pnpm/symlink-dependency@4.0.0
  - @pnpm/types@7.0.0

## 0.45.4

### Patch Changes

- @pnpm/lockfile-to-pnp@0.3.25
- @pnpm/headless@15.0.3

## 0.45.3

### Patch Changes

- Updated dependencies [d853fb14a]
- Updated dependencies [d853fb14a]
- Updated dependencies [d853fb14a]
  - @pnpm/lifecycle@9.6.5
  - @pnpm/link-bins@5.3.25
  - @pnpm/read-package-json@4.0.0
  - @pnpm/remove-bins@1.0.12
  - @pnpm/build-modules@5.2.12
  - @pnpm/headless@15.0.2
  - @pnpm/hoist@4.0.26
  - @pnpm/lockfile-to-pnp@0.3.24
  - @pnpm/package-requester@13.0.1
  - @pnpm/resolve-dependencies@19.0.2
  - @pnpm/modules-cleaner@10.0.23

## 0.45.2

### Patch Changes

- Updated dependencies [6350a3381]
  - @pnpm/link-bins@5.3.24
  - @pnpm/build-modules@5.2.11
  - @pnpm/headless@15.0.1
  - @pnpm/hoist@4.0.25
  - @pnpm/package-requester@13.0.0
  - @pnpm/resolve-dependencies@19.0.1

## 0.45.1

### Patch Changes

- @pnpm/headless@15.0.0

## 0.45.0

### Minor Changes

- 8d1dfa89c: Breaking changes to the store controller API.

  The options to `requestPackage()` and `fetchPackage()` changed.

### Patch Changes

- f008425cd: Fix the lockfile if it contains invalid checksums.
- Updated dependencies [8d1dfa89c]
  - @pnpm/headless@15.0.0
  - @pnpm/package-requester@13.0.0
  - @pnpm/store-controller-types@10.0.0
  - @pnpm/resolve-dependencies@19.0.0
  - @pnpm/build-modules@5.2.10
  - @pnpm/modules-cleaner@10.0.22
  - @pnpm/lockfile-to-pnp@0.3.23

## 0.44.8

### Patch Changes

- Updated dependencies [ef1588413]
  - @pnpm/resolve-dependencies@18.3.3

## 0.44.7

### Patch Changes

- Updated dependencies [51e1456dd]
- Updated dependencies [51e1456dd]
  - @pnpm/lockfile-file@3.2.1
  - @pnpm/get-context@4.0.0
  - @pnpm/headless@14.6.10
  - @pnpm/lockfile-to-pnp@0.3.22

## 0.44.6

### Patch Changes

- Updated dependencies [27a40321c]
  - @pnpm/get-context@3.3.6
  - @pnpm/headless@14.6.9
  - @pnpm/package-requester@12.2.2

## 0.44.5

### Patch Changes

- Updated dependencies [a78e5c47f]
  - @pnpm/link-bins@5.3.23
  - @pnpm/build-modules@5.2.9
  - @pnpm/headless@14.6.8
  - @pnpm/hoist@4.0.24

## 0.44.4

### Patch Changes

- Updated dependencies [249c068dd]
  - @pnpm/resolve-dependencies@18.3.2
  - @pnpm/lockfile-to-pnp@0.3.21
  - @pnpm/headless@14.6.7

## 0.44.3

### Patch Changes

- ad113645b: pin graceful-fs to v4.2.4
- Updated dependencies [ad113645b]
  - @pnpm/read-project-manifest@1.1.7
  - @pnpm/link-bins@5.3.22
  - @pnpm/remove-bins@1.0.11
  - @pnpm/headless@14.6.6
  - @pnpm/lockfile-to-pnp@0.3.20
  - @pnpm/build-modules@5.2.8
  - @pnpm/hoist@4.0.23
  - @pnpm/modules-cleaner@10.0.21
  - @pnpm/package-requester@12.2.2

## 0.44.2

### Patch Changes

- @pnpm/lockfile-to-pnp@0.3.19
- @pnpm/headless@14.6.5

## 0.44.1

### Patch Changes

- 9a9bc67d2: Don't crash if the CLI manifest is not found.
- Updated dependencies [7578a5ad4]
- Updated dependencies [9a9bc67d2]
  - @pnpm/resolve-dependencies@18.3.1
  - @pnpm/lifecycle@9.6.4
  - @pnpm/build-modules@5.2.7
  - @pnpm/headless@14.6.4

## 0.44.0

### Minor Changes

- 9ad8c27bf: Allow to ignore builds of specified dependencies through the `pnpm.neverBuiltDependencies` field in `package.json`.

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-file@3.2.0
  - @pnpm/resolve-dependencies@18.3.0
  - @pnpm/types@6.4.0
  - @pnpm/get-context@3.3.5
  - @pnpm/headless@14.6.3
  - @pnpm/lockfile-to-pnp@0.3.18
  - @pnpm/filter-lockfile@4.0.17
  - @pnpm/hoist@4.0.22
  - @pnpm/lockfile-utils@2.0.22
  - @pnpm/lockfile-walker@3.0.9
  - @pnpm/modules-cleaner@10.0.20
  - @pnpm/prune-lockfile@2.0.19
  - @pnpm/build-modules@5.2.6
  - @pnpm/core-loggers@5.0.3
  - dependency-path@5.1.1
  - @pnpm/lifecycle@9.6.3
  - @pnpm/link-bins@5.3.21
  - @pnpm/manifest-utils@1.1.5
  - @pnpm/modules-yaml@8.0.6
  - @pnpm/normalize-registries@1.0.6
  - @pnpm/package-requester@12.2.2
  - @pnpm/read-package-json@3.1.9
  - @pnpm/read-project-manifest@1.1.6
  - @pnpm/remove-bins@1.0.10
  - @pnpm/resolver-base@7.1.1
  - @pnpm/store-controller-types@9.2.1
  - @pnpm/symlink-dependency@3.0.13

## 0.43.29

### Patch Changes

- Updated dependencies [1c851f2a6]
  - @pnpm/headless@14.6.2
  - @pnpm/lockfile-to-pnp@0.3.17

## 0.43.28

### Patch Changes

- af897c324: Resolution should never be skipped if the overrides were updated.
- af897c324: Installation should fail if the overrides in the lockfile don't match the ones in the package.json and the frozenLockfile option is on.
- Updated dependencies [af897c324]
- Updated dependencies [af897c324]
  - @pnpm/filter-lockfile@4.0.16
  - @pnpm/lockfile-file@3.1.4
  - @pnpm/headless@14.6.1
  - @pnpm/modules-cleaner@10.0.19
  - @pnpm/get-context@3.3.4
  - @pnpm/lockfile-to-pnp@0.3.16

## 0.43.27

### Patch Changes

- Updated dependencies [e665f5105]
  - @pnpm/resolve-dependencies@18.2.6

## 0.43.26

### Patch Changes

- f40bc5927: New option added: enableModulesDir. When `false`, pnpm will not write any files to the modules directory. This is useful for when you want to mount the modules directory with FUSE.
- 672c27cfe: Don't create broken symlinks to skipped optional dependencies, when hoisting. This issue was already fixed in pnpm v5.13.7 for the case when the lockfile is up-to-date. This fixes the same issue for cases when the lockfile need updates. For instance, when adding a new package.
- Updated dependencies [1e4a3a17a]
- Updated dependencies [f40bc5927]
- Updated dependencies [d5ef7958a]
  - @pnpm/lockfile-file@3.1.3
  - @pnpm/headless@14.6.0
  - @pnpm/get-context@3.3.3
  - @pnpm/lockfile-to-pnp@0.3.15

## 0.43.25

### Patch Changes

- Updated dependencies [db0c7e157]
- Updated dependencies [4d64969a6]
  - @pnpm/resolve-dependencies@18.2.5

## 0.43.24

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0
  - @pnpm/filter-lockfile@4.0.15
  - @pnpm/headless@14.5.15
  - @pnpm/hoist@4.0.21
  - @pnpm/lockfile-to-pnp@0.3.14
  - @pnpm/lockfile-utils@2.0.21
  - @pnpm/lockfile-walker@3.0.8
  - @pnpm/modules-cleaner@10.0.18
  - @pnpm/package-requester@12.2.1
  - @pnpm/prune-lockfile@2.0.18
  - @pnpm/resolve-dependencies@18.2.4

## 0.43.23

### Patch Changes

- @pnpm/lockfile-to-pnp@0.3.13
- @pnpm/headless@14.5.14

## 0.43.22

### Patch Changes

- @pnpm/lockfile-to-pnp@0.3.12
- @pnpm/headless@14.5.13

## 0.43.21

### Patch Changes

- Updated dependencies [d064b7736]
- Updated dependencies [130970393]
- Updated dependencies [130970393]
  - @pnpm/headless@14.5.12
  - @pnpm/modules-cleaner@10.0.17
  - @pnpm/lockfile-to-pnp@0.3.11

## 0.43.20

### Patch Changes

- @pnpm/resolve-dependencies@18.2.3
- @pnpm/headless@14.5.11
- @pnpm/package-requester@12.2.0

## 0.43.19

### Patch Changes

- Updated dependencies [fba715512]
  - @pnpm/lockfile-file@3.1.2
  - @pnpm/get-context@3.3.2
  - @pnpm/headless@14.5.10
  - @pnpm/lockfile-to-pnp@0.3.10
  - @pnpm/package-requester@12.2.0
  - @pnpm/resolve-dependencies@18.2.2

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

- 95ad9cafa: Install should fail if there are references to a pruned workspace project.

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
