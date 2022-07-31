# @pnpm/lifecycle

## 13.1.4

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/core-loggers@7.0.6
  - @pnpm/read-package-json@6.0.7
  - @pnpm/store-controller-types@14.0.2
  - @pnpm/directory-fetcher@3.0.9

## 13.1.3

### Patch Changes

- @pnpm/directory-fetcher@3.0.8

## 13.1.2

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/directory-fetcher@3.0.7

## 13.1.1

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/core-loggers@7.0.5
  - @pnpm/read-package-json@6.0.6
  - @pnpm/store-controller-types@14.0.1
  - @pnpm/directory-fetcher@3.0.6

## 13.1.0

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
- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/read-package-json@6.0.5
  - @pnpm/directory-fetcher@3.0.5

## 13.0.5

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/core-loggers@7.0.3
  - @pnpm/read-package-json@6.0.4
  - @pnpm/store-controller-types@13.0.4
  - @pnpm/directory-fetcher@3.0.4

## 13.0.4

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2
  - @pnpm/read-package-json@6.0.3
  - @pnpm/store-controller-types@13.0.3
  - @pnpm/directory-fetcher@3.0.3

## 13.0.3

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/store-controller-types@13.0.2
  - @pnpm/directory-fetcher@3.0.2

## 13.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - @pnpm/read-package-json@6.0.2
  - @pnpm/store-controller-types@13.0.1
  - @pnpm/directory-fetcher@3.0.1

## 13.0.1

### Patch Changes

- Updated dependencies [41cae6450]
  - @pnpm/directory-fetcher@3.0.0
  - @pnpm/read-package-json@6.0.1

## 13.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.
- d999a0801: Allow to execute a lifecycle script in a directory that doesn't match the package's name. Previously this was only allowed with the `--unsafe-perm` CLI option [#3709](https://github.com/pnpm/pnpm/issues/3709).

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/directory-fetcher@2.0.0
  - @pnpm/read-package-json@6.0.0
  - @pnpm/store-controller-types@13.0.0

## 12.1.7

### Patch Changes

- Updated dependencies [5c525db13]
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/read-package-json@5.0.12
  - @pnpm/directory-fetcher@1.0.7

## 12.1.6

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/core-loggers@6.1.4
  - @pnpm/read-package-json@5.0.11
  - @pnpm/store-controller-types@11.0.12
  - @pnpm/directory-fetcher@1.0.6

## 12.1.5

### Patch Changes

- 7ae349cd3: `node_modules` directories inside injected dependencies should not be overwritten.

## 12.1.4

### Patch Changes

- Updated dependencies [aa1f9dc19]
- Updated dependencies [4f78a2a5f]
- Updated dependencies [26cd01b88]
  - @pnpm/directory-fetcher@1.0.5
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - @pnpm/read-package-json@5.0.10
  - @pnpm/store-controller-types@11.0.11

## 12.1.3

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - @pnpm/read-package-json@5.0.9
  - @pnpm/store-controller-types@11.0.10
  - @pnpm/directory-fetcher@1.0.4

## 12.1.2

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - @pnpm/read-package-json@5.0.8
  - @pnpm/store-controller-types@11.0.9
  - @pnpm/directory-fetcher@1.0.3

## 12.1.1

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/read-package-json@5.0.7
  - @pnpm/store-controller-types@11.0.8
  - @pnpm/directory-fetcher@1.0.2

## 12.1.0

### Minor Changes

- 002778559: New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

## 12.0.2

### Patch Changes

- fa03cbdc8: Escape the arguments that are passed to the scripts [#3907](https://github.com/pnpm/pnpm/issues/3907).
- Updated dependencies [108bd4a39]
- Updated dependencies [302ae4f6f]
  - @pnpm/directory-fetcher@1.0.1
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6
  - @pnpm/read-package-json@5.0.6
  - @pnpm/store-controller-types@11.0.7

## 12.0.1

### Patch Changes

- 5b90ab98f: Do not index the project directory if it should not be hard linked to any other project [#3949](https://github.com/pnpm/pnpm/issues/3949).

## 12.0.0

### Major Changes

- 4ab87844a: `storeController` is a required new option of `runLifecycleHooksConcurrently()`.

### Minor Changes

- 4ab87844a: `runLifecycleHooksConcurrently` will relink projects after rebuilding them if they are injected to other projects.

### Patch Changes

- 37dcfceeb: Buffer warnings fixed [#3932](https://github.com/pnpm/pnpm/issues/3932).
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/directory-fetcher@1.0.0
  - @pnpm/core-loggers@6.0.5
  - @pnpm/read-package-json@5.0.5
  - @pnpm/store-controller-types@11.0.6

## 11.0.5

### Patch Changes

- 4a4d42d8f: Packages that have no `package.json` files should be skipped.

## 11.0.4

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - @pnpm/read-package-json@5.0.4

## 11.0.3

### Patch Changes

- 7af16a011: Print a warning, when a lifecycle script is skipped.

## 11.0.2

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - @pnpm/read-package-json@5.0.3

## 11.0.1

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2
  - @pnpm/read-package-json@5.0.2

## 11.0.0

### Major Changes

- e6a2654a2: `prepare` scripts of Git-hosted packages are not executed (they are executed during fetching by `@pnpm/git-fetcher`).

## 10.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1
  - @pnpm/read-package-json@5.0.1

## 10.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
  - @pnpm/core-loggers@6.0.0
  - @pnpm/read-package-json@5.0.0
  - @pnpm/types@7.0.0

## 9.6.5

### Patch Changes

- d853fb14a: Run `node-gyp` when `binding.gyp` is present, even if an install lifecycle script is not present in the scripts field.
- Updated dependencies [d853fb14a]
  - @pnpm/read-package-json@4.0.0

## 9.6.4

### Patch Changes

- 9a9bc67d2: It should be possible to run pnpm using only its bundled file.

## 9.6.3

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/core-loggers@5.0.3
  - @pnpm/read-package-json@3.1.9

## 9.6.2

### Patch Changes

- @pnpm/read-package-json@3.1.8

## 9.6.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/core-loggers@5.0.2
  - @pnpm/read-package-json@3.1.7

## 9.6.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

## 9.5.1

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [212671848]
  - @pnpm/types@6.3.0
  - @pnpm/read-package-json@3.1.6
  - @pnpm/core-loggers@5.0.1

## 9.5.0

### Minor Changes

- f591fdeeb: New option added: extraEnv. extraEnv allows to pass environment variables that will be set for the child process.
- f591fdeeb: New function exported: `makeNodeRequireOption()`.

## 9.4.0

### Minor Changes

- 203e65ac8: A new option added to set the INIT_CWD env variable for scripts: opts.initCwd.

## 9.3.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

## 9.2.5

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0

## 9.2.4

### Patch Changes

- @pnpm/read-package-json@3.1.5

## 9.2.3

### Patch Changes

- Updated dependencies [9f5803187]
  - @pnpm/read-package-json@3.1.4

## 9.2.2

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 9.2.1

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 9.2.0

### Minor Changes

- 76aaead32: Added an option for silent execution: opts.silent.

## 9.1.3

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/read-package-json@3.1.3

## 9.1.2

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - @pnpm/read-package-json@3.1.2

## 9.1.1

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- 68d8dc68f: Update node-gyp to v7.
- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0

## 9.1.0

### Minor Changes

- 8094b2a62: Run lifecycle scripts with the PNPM_SCRIPT_SRC_DIR env variable set. This new env variable contains the directory of the package.json file that contains the executed lifecycle script.

## 9.0.0

### Major Changes

- e3990787a: Rename NodeModules to Modules in option names.

### Minor Changes

- f35a3ec1c: Don't execute lifecycle scripts that are meant to prevent the usage of npm or Yarn.

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/read-package-json@3.1.1

## 9.0.0-alpha.1

### Major Changes

- e3990787: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0

## 8.2.0-alpha.0

### Minor Changes

- f35a3ec1c: Don't execute lifecycle scripts that are meant to prevent the usage of npm or Yarn.

## 8.2.0

### Minor Changes

- 2ec4c4eb9: Don't execute lifecycle scripts that are meant to prevent the usage of npm or Yarn.
