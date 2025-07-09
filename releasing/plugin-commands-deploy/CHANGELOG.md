# @pnpm/plugin-commands-deploy

## 1001.1.30

### Patch Changes

- Updated dependencies [7ad0bc3]
  - @pnpm/cli-utils@1001.0.1
  - @pnpm/plugin-commands-installation@1004.3.1

## 1001.1.29

### Patch Changes

- Updated dependencies [623da6f]
- Updated dependencies [cf630a8]
- Updated dependencies [e225310]
  - @pnpm/config@1004.1.0
  - @pnpm/cli-utils@1001.0.0
  - @pnpm/plugin-commands-installation@1004.3.0
  - @pnpm/dependency-path@1001.0.1
  - @pnpm/lockfile.fs@1001.1.15

## 1001.1.28

### Patch Changes

- Updated dependencies [b511eac]
  - @pnpm/plugin-commands-installation@1004.2.2

## 1001.1.27

### Patch Changes

- @pnpm/plugin-commands-installation@1004.2.1

## 1001.1.26

### Patch Changes

- 983efdc: Fix a bug in which `pnpm deploy` fails due to overridden dependencies having peer dependencies causing `ERR_PNPM_OUTDATED_LOCKFILE` [#9595](https://github.com/pnpm/pnpm/issues/9595).
- Updated dependencies [983efdc]
- Updated dependencies [540986f]
  - @pnpm/plugin-commands-installation@1004.2.0
  - @pnpm/dependency-path@1001.0.0
  - @pnpm/lockfile.fs@1001.1.14
  - @pnpm/cli-utils@1000.1.7

## 1001.1.25

### Patch Changes

- 1959f99: Revert [#9574](https://github.com/pnpm/pnpm/pull/9574) to fix a regression [#9596](https://github.com/pnpm/pnpm/issues/9596).
- Updated dependencies [b217bbb]
- Updated dependencies [b0ead51]
- Updated dependencies [c8341cc]
- Updated dependencies [b0ead51]
- Updated dependencies [046af72]
  - @pnpm/plugin-commands-installation@1004.1.0
  - @pnpm/config@1004.0.0
  - @pnpm/directory-fetcher@1000.1.8
  - @pnpm/cli-utils@1000.1.6
  - @pnpm/lockfile.fs@1001.1.13
  - @pnpm/fs.indexed-pkg-importer@1000.1.9

## 1001.1.24

### Patch Changes

- 3387aa9: Fix an issue in which `pnpm deploy --legacy` creates unexpected directories when the root `package.json` has a workspace package as a peer dependency [#9550](https://github.com/pnpm/pnpm/issues/9550).
- 3f268ff: Let `pnpm deploy` work in repos with `overrides` when `inject-workspace-packages=true` [#9283](https://github.com/pnpm/pnpm/issues/9283).
- Updated dependencies [8d175c0]
  - @pnpm/config@1003.1.1
  - @pnpm/plugin-commands-installation@1004.0.3
  - @pnpm/cli-utils@1000.1.5
  - @pnpm/fs.indexed-pkg-importer@1000.1.8

## 1001.1.23

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [b282bd1]
- Updated dependencies [fdb1d98]
- Updated dependencies [e4af08c]
- Updated dependencies [09cf46f]
- Updated dependencies [36d1448]
- Updated dependencies [9362b5f]
- Updated dependencies [5ec7255]
- Updated dependencies [6cf010c]
  - @pnpm/config@1003.1.0
  - @pnpm/plugin-commands-installation@1004.0.2
  - @pnpm/directory-fetcher@1000.1.7
  - @pnpm/fs.indexed-pkg-importer@1000.1.7
  - @pnpm/cli-utils@1000.1.4
  - @pnpm/lockfile.fs@1001.1.12
  - @pnpm/types@1000.6.0
  - @pnpm/lockfile.types@1001.0.8
  - @pnpm/dependency-path@1000.0.9

## 1001.1.22

### Patch Changes

- Updated dependencies [7c7f0d6]
  - @pnpm/common-cli-options-help@1000.0.1
  - @pnpm/plugin-commands-installation@1004.0.1
  - @pnpm/cli-utils@1000.1.3
  - @pnpm/config@1003.0.1

## 1001.1.21

### Patch Changes

- Updated dependencies [56bb69b]
- Updated dependencies [8a9f3a4]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/config@1003.0.0
  - @pnpm/plugin-commands-installation@1004.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/types@1000.5.0
  - @pnpm/cli-utils@1000.1.2
  - @pnpm/fs.indexed-pkg-importer@1000.1.6
  - @pnpm/directory-fetcher@1000.1.6
  - @pnpm/lockfile.fs@1001.1.11
  - @pnpm/lockfile.types@1001.0.7
  - @pnpm/dependency-path@1000.0.8

## 1001.1.20

### Patch Changes

- Updated dependencies [032fff8]
- Updated dependencies [4d95e93]
  - @pnpm/fs.indexed-pkg-importer@1000.1.5
  - @pnpm/plugin-commands-installation@1003.0.1
  - @pnpm/directory-fetcher@1000.1.5
  - @pnpm/cli-utils@1000.1.1
  - @pnpm/lockfile.fs@1001.1.10
  - @pnpm/config@1002.7.2

## 1001.1.19

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [5679712]
- Updated dependencies [01f2bcf]
- Updated dependencies [750ae7d]
- Updated dependencies [8033854]
- Updated dependencies [1413c25]
  - @pnpm/types@1000.4.0
  - @pnpm/config@1002.7.1
  - @pnpm/plugin-commands-installation@1003.0.0
  - @pnpm/cli-utils@1000.1.0
  - @pnpm/directory-fetcher@1000.1.4
  - @pnpm/lockfile.fs@1001.1.9
  - @pnpm/lockfile.types@1001.0.6
  - @pnpm/dependency-path@1000.0.7
  - @pnpm/fs.indexed-pkg-importer@1000.1.4

## 1001.1.18

### Patch Changes

- Updated dependencies [e57f1df]
  - @pnpm/config@1002.7.0
  - @pnpm/cli-utils@1000.0.19
  - @pnpm/plugin-commands-installation@1002.2.4

## 1001.1.17

### Patch Changes

- 9d30085: Removed a defunct special case to handle the `catalog:` protocol when deploying a package. This is no longer necessary with newer version of pnpm which handle injected workspace packages using the `catalog:` protocol out of the box.
- Updated dependencies [9bcca9f]
- Updated dependencies [5b35dff]
- Updated dependencies [9bcca9f]
- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/config@1002.6.0
  - @pnpm/types@1000.3.0
  - @pnpm/cli-utils@1000.0.18
  - @pnpm/plugin-commands-installation@1002.2.3
  - @pnpm/lockfile.types@1001.0.5
  - @pnpm/directory-fetcher@1000.1.3
  - @pnpm/lockfile.fs@1001.1.8
  - @pnpm/dependency-path@1000.0.6
  - @pnpm/fs.indexed-pkg-importer@1000.1.3

## 1001.1.16

### Patch Changes

- Updated dependencies [936430a]
  - @pnpm/config@1002.5.4
  - @pnpm/plugin-commands-installation@1002.2.2
  - @pnpm/cli-utils@1000.0.17
  - @pnpm/directory-fetcher@1000.1.2
  - @pnpm/lockfile.fs@1001.1.7
  - @pnpm/fs.indexed-pkg-importer@1000.1.2

## 1001.1.15

### Patch Changes

- Updated dependencies [e5b7bf4]
  - @pnpm/plugin-commands-installation@1002.2.1

## 1001.1.14

### Patch Changes

- cda1c43: An internal refactor was made to the `deploy` command to better handle a change in `plugin-commands-installation` when `node-linker=hoisted`. There are no behavior changes expected with this refactor.
- Updated dependencies [b4efd0e]
- Updated dependencies [6e4459c]
- Updated dependencies [cda1c43]
  - @pnpm/plugin-commands-installation@1002.2.0
  - @pnpm/config@1002.5.3
  - @pnpm/cli-utils@1000.0.16

## 1001.1.13

### Patch Changes

- @pnpm/cli-utils@1000.0.15
- @pnpm/plugin-commands-installation@1002.1.2
- @pnpm/dependency-path@1000.0.5
- @pnpm/config@1002.5.2
- @pnpm/lockfile.fs@1001.1.6

## 1001.1.12

### Patch Changes

- Updated dependencies [c3aa4d8]
  - @pnpm/config@1002.5.1
  - @pnpm/cli-utils@1000.0.14
  - @pnpm/plugin-commands-installation@1002.1.1

## 1001.1.11

### Patch Changes

- f5940cc: `pnpm deploy` should not remove fields from the deployed package's `package.json` file [#9215](https://github.com/pnpm/pnpm/issues/9215).
- a5e4965: Fix `pnpm deploy` creating a `package.json` without the `imports` and `license` field [#9193](https://github.com/pnpm/pnpm/issues/9193).
- e4eeafd: Fix a bug causing entries in the `catalogs` section of the `pnpm-lock.yaml` file to be removed when `dedupe-peer-dependents=false` on a filtered install. [#9112](https://github.com/pnpm/pnpm/issues/9112)
- Updated dependencies [6a59366]
- Updated dependencies [a5e4965]
- Updated dependencies [d9d7607]
- Updated dependencies [d965748]
- Updated dependencies [e4eeafd]
  - @pnpm/plugin-commands-installation@1002.1.0
  - @pnpm/types@1000.2.1
  - @pnpm/config@1002.5.0
  - @pnpm/dependency-path@1000.0.4
  - @pnpm/cli-utils@1000.0.13
  - @pnpm/directory-fetcher@1000.1.1
  - @pnpm/lockfile.fs@1001.1.5
  - @pnpm/lockfile.types@1001.0.4
  - @pnpm/fs.indexed-pkg-importer@1000.1.1

## 1001.1.10

### Patch Changes

- Updated dependencies [76973d8]
- Updated dependencies [1c2eb8c]
  - @pnpm/plugin-commands-installation@1002.0.1
  - @pnpm/config@1002.4.1
  - @pnpm/cli-utils@1000.0.12

## 1001.1.9

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
- Updated dependencies [e32b1a2]
- Updated dependencies [5296961]
- Updated dependencies [8fcc221]
- Updated dependencies [e32b1a2]
  - @pnpm/plugin-commands-installation@1002.0.0
  - @pnpm/config@1002.4.0
  - @pnpm/fs.indexed-pkg-importer@1000.1.0
  - @pnpm/types@1000.2.0
  - @pnpm/directory-fetcher@1000.1.0
  - @pnpm/cli-utils@1000.0.11
  - @pnpm/lockfile.fs@1001.1.4
  - @pnpm/lockfile.types@1001.0.3
  - @pnpm/dependency-path@1000.0.3

## 1001.1.8

### Patch Changes

- Updated dependencies [fee898f]
- Updated dependencies [546ab37]
  - @pnpm/config@1002.3.1
  - @pnpm/plugin-commands-installation@1001.5.1
  - @pnpm/cli-utils@1000.0.10
  - @pnpm/lockfile.fs@1001.1.3

## 1001.1.7

### Patch Changes

- Updated dependencies [91d46ee]
  - @pnpm/plugin-commands-installation@1001.5.0
  - @pnpm/cli-utils@1000.0.9

## 1001.1.6

### Patch Changes

- Updated dependencies [f6006f2]
  - @pnpm/plugin-commands-installation@1001.4.0
  - @pnpm/config@1002.3.0
  - @pnpm/cli-utils@1000.0.8

## 1001.1.5

### Patch Changes

- 96cdfa3: `pnpm deploy --legacy` should work without injected dependencies.
- e9d8f48: Add information about how to deploy without "injected dependencies" to the "pnpm deploy" error message.
  - @pnpm/plugin-commands-installation@1001.3.2

## 1001.1.4

### Patch Changes

- a08a5a9: Fix a bug in which `pnpm deploy` fails to read the correct `projectId` when the deploy source is the same as the workspace directory [#9001](https://github.com/pnpm/pnpm/issues/9001).
  - @pnpm/cli-utils@1000.0.7
  - @pnpm/config@1002.2.1
  - @pnpm/directory-fetcher@1000.0.5
  - @pnpm/plugin-commands-installation@1001.3.1

## 1001.1.3

### Patch Changes

- 9a44e6c: `pnpm deploy` should inherit the `pnpm` object from the root `package.json` [#8991](https://github.com/pnpm/pnpm/pull/8991).
- b562deb: Fix `pnpm deploy` creating a package.json without the `"type"` key [#8962](https://github.com/pnpm/pnpm/issues/8962).
- Updated dependencies [9a44e6c]
- Updated dependencies [b562deb]
- Updated dependencies [f3ffaed]
- Updated dependencies [c96eb2b]
  - @pnpm/constants@1001.1.0
  - @pnpm/types@1000.1.1
  - @pnpm/plugin-commands-installation@1001.3.0
  - @pnpm/config@1002.2.0
  - @pnpm/lockfile.fs@1001.1.2
  - @pnpm/error@1000.0.2
  - @pnpm/cli-utils@1000.0.6
  - @pnpm/directory-fetcher@1000.0.4
  - @pnpm/lockfile.types@1001.0.2
  - @pnpm/dependency-path@1000.0.2
  - @pnpm/catalogs.resolver@1000.0.2
  - @pnpm/fs.indexed-pkg-importer@1000.0.5

## 1001.1.2

### Patch Changes

- Updated dependencies [e050221]
  - @pnpm/plugin-commands-installation@1001.2.1
  - @pnpm/cli-utils@1000.0.5
  - @pnpm/config@1002.1.2
  - @pnpm/directory-fetcher@1000.0.3
  - @pnpm/fs.indexed-pkg-importer@1000.0.4

## 1001.1.1

### Patch Changes

- Updated dependencies [c7eefdd]
- Updated dependencies [9591a18]
- Updated dependencies [1f5169f]
  - @pnpm/plugin-commands-installation@1001.2.0
  - @pnpm/types@1000.1.0
  - @pnpm/config@1002.1.1
  - @pnpm/cli-utils@1000.0.4
  - @pnpm/directory-fetcher@1000.0.2
  - @pnpm/lockfile.fs@1001.1.1
  - @pnpm/lockfile.types@1001.0.1
  - @pnpm/dependency-path@1000.0.1
  - @pnpm/fs.indexed-pkg-importer@1000.0.3

## 1001.1.0

### Minor Changes

- f891288: `pnpm deploy` now tries creating a dedicated lockfile from a shared lockfile for deployment. It will fallback to deployment without a lockfile if there is no shared lockfile or `force-legacy-deploy` is set to `true`.

### Patch Changes

- f891288: Fix an issue in which `pnpm deploy --prod` fails due to missing `devDependencies` [#8778](https://github.com/pnpm/pnpm/issues/8778).
- Updated dependencies [f90a94b]
- Updated dependencies [f891288]
- Updated dependencies [f891288]
  - @pnpm/config@1002.1.0
  - @pnpm/plugin-commands-installation@1001.1.0
  - @pnpm/cli-utils@1000.0.3

## 1001.0.2

### Patch Changes

- Updated dependencies [f685565]
  - @pnpm/plugin-commands-installation@1001.0.2
  - @pnpm/fs.indexed-pkg-importer@1000.0.2
  - @pnpm/cli-utils@1000.0.2

## 1001.0.1

### Patch Changes

- @pnpm/plugin-commands-installation@1001.0.1

## 1001.0.0

### Major Changes

- ac5b9d8: All dependencies are installed even when the `NODE_ENV` environment variable is set to `production [#8827](https://github.com/pnpm/pnpm/issues/8827).
- 31911f1: The deploy command works only in workspaces that use the `inject-workspace-packages=true` setting.

### Patch Changes

- Updated dependencies [ac5b9d8]
- Updated dependencies [31911f1]
- Updated dependencies [b8bda0a]
- Updated dependencies [d47c426]
- Updated dependencies [a76da0c]
  - @pnpm/plugin-commands-installation@1001.0.0
  - @pnpm/cli-utils@1000.0.1
  - @pnpm/error@1000.0.1
  - @pnpm/fs.indexed-pkg-importer@1000.0.1
  - @pnpm/directory-fetcher@1000.0.1
  - @pnpm/catalogs.resolver@1000.0.1

## 5.1.32

### Patch Changes

- Updated dependencies [477e0c1]
- Updated dependencies [19d5b51]
- Updated dependencies [6b27c81]
  - @pnpm/plugin-commands-installation@18.0.0
  - @pnpm/error@6.0.3
  - @pnpm/cli-utils@4.0.8
  - @pnpm/catalogs.resolver@0.1.2
  - @pnpm/fs.indexed-pkg-importer@6.0.9
  - @pnpm/directory-fetcher@8.0.10

## 5.1.31

### Patch Changes

- Updated dependencies [6014522]
  - @pnpm/plugin-commands-installation@17.2.7
  - @pnpm/cli-utils@4.0.7
  - @pnpm/fs.indexed-pkg-importer@6.0.9
  - @pnpm/directory-fetcher@8.0.9

## 5.1.30

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.6
- @pnpm/cli-utils@4.0.6
- @pnpm/fs.indexed-pkg-importer@6.0.9
- @pnpm/directory-fetcher@8.0.9

## 5.1.29

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.5

## 5.1.28

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/plugin-commands-installation@17.2.4
  - @pnpm/error@6.0.2
  - @pnpm/cli-utils@4.0.6
  - @pnpm/catalogs.resolver@0.1.1
  - @pnpm/fs.indexed-pkg-importer@6.0.9
  - @pnpm/directory-fetcher@8.0.9

## 5.1.27

### Patch Changes

- ad1fd64: Fix a regression in which `pnpm deploy` with `node-linker=hoisted` produces an empty `node_modules` directory [#6682](https://github.com/pnpm/pnpm/issues/6682).
- eeb76cd: `pnpm deploy` should work in workspace with `shared-workspace-lockfile=false` [#8475](https://github.com/pnpm/pnpm/issues/8475).
- Updated dependencies [ad1fd64]
- Updated dependencies [eeb76cd]
  - @pnpm/plugin-commands-installation@17.2.3

## 5.1.26

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0
  - @pnpm/cli-utils@4.0.5
  - @pnpm/directory-fetcher@8.0.8
  - @pnpm/plugin-commands-installation@17.2.2
  - @pnpm/fs.indexed-pkg-importer@6.0.9

## 5.1.25

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.1

## 5.1.24

### Patch Changes

- 7ee59a1: `pnpm deploy` should write the `node_modules/.modules.yaml` to the `node_modules` directory within the deploy directory [#7731](https://github.com/pnpm/pnpm/issues/7731).
- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/plugin-commands-installation@17.2.0
  - @pnpm/cli-utils@4.0.4
  - @pnpm/directory-fetcher@8.0.7
  - @pnpm/fs.indexed-pkg-importer@6.0.8

## 5.1.23

### Patch Changes

- @pnpm/plugin-commands-installation@17.1.1

## 5.1.22

### Patch Changes

- Updated dependencies [eb8bf2a]
  - @pnpm/plugin-commands-installation@17.1.0
  - @pnpm/cli-utils@4.0.3

## 5.1.21

### Patch Changes

- @pnpm/cli-utils@4.0.2
- @pnpm/plugin-commands-installation@17.0.7

## 5.1.20

### Patch Changes

- @pnpm/plugin-commands-installation@17.0.6

## 5.1.19

### Patch Changes

- @pnpm/plugin-commands-installation@17.0.5

## 5.1.18

### Patch Changes

- @pnpm/cli-utils@4.0.1
- @pnpm/plugin-commands-installation@17.0.4

## 5.1.17

### Patch Changes

- Updated dependencies [26b065c]
  - @pnpm/cli-utils@4.0.0
  - @pnpm/plugin-commands-installation@17.0.3

## 5.1.16

### Patch Changes

- Updated dependencies [cb006df]
- Updated dependencies [98c8bd6]
  - @pnpm/types@12.0.0
  - @pnpm/cli-utils@3.1.7
  - @pnpm/plugin-commands-installation@17.0.2
  - @pnpm/directory-fetcher@8.0.6
  - @pnpm/fs.indexed-pkg-importer@6.0.7

## 5.1.15

### Patch Changes

- @pnpm/plugin-commands-installation@17.0.1
- @pnpm/cli-utils@3.1.6
- @pnpm/fs.indexed-pkg-importer@6.0.6
- @pnpm/directory-fetcher@8.0.5

## 5.1.14

### Patch Changes

- 1e4dd79: The `pnpm deploy` command now supports the [`catalog:` protocol](https://pnpm.io/catalogs).
- Updated dependencies [1e4dd79]
- Updated dependencies [0ef168b]
  - @pnpm/plugin-commands-installation@17.0.0
  - @pnpm/types@11.1.0
  - @pnpm/cli-utils@3.1.5
  - @pnpm/directory-fetcher@8.0.5
  - @pnpm/fs.indexed-pkg-importer@6.0.6

## 5.1.13

### Patch Changes

- Updated dependencies [afe520d]
- Updated dependencies [afe520d]
  - @pnpm/fs.indexed-pkg-importer@6.0.5
  - @pnpm/plugin-commands-installation@16.0.1
  - @pnpm/cli-utils@3.1.4
  - @pnpm/directory-fetcher@8.0.4

## 5.1.12

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
- Updated dependencies [84654bd]
  - @pnpm/plugin-commands-installation@16.0.0
  - @pnpm/types@11.0.0
  - @pnpm/cli-utils@3.1.3
  - @pnpm/directory-fetcher@8.0.4
  - @pnpm/fs.indexed-pkg-importer@6.0.4

## 5.1.11

### Patch Changes

- Updated dependencies [13e55b2]
- Updated dependencies [04b8363]
  - @pnpm/plugin-commands-installation@15.1.11
  - @pnpm/types@10.1.1
  - @pnpm/cli-utils@3.1.2
  - @pnpm/directory-fetcher@8.0.3
  - @pnpm/fs.indexed-pkg-importer@6.0.3

## 5.1.10

### Patch Changes

- @pnpm/plugin-commands-installation@15.1.10
- @pnpm/cli-utils@3.1.1
- @pnpm/fs.indexed-pkg-importer@6.0.2
- @pnpm/directory-fetcher@8.0.2

## 5.1.9

### Patch Changes

- Updated dependencies [b7ca13f]
- Updated dependencies [b7ca13f]
  - @pnpm/cli-utils@3.1.0
  - @pnpm/plugin-commands-installation@15.1.9

## 5.1.8

### Patch Changes

- @pnpm/plugin-commands-installation@15.1.8

## 5.1.7

### Patch Changes

- @pnpm/plugin-commands-installation@15.1.7

## 5.1.6

### Patch Changes

- @pnpm/plugin-commands-installation@15.1.6

## 5.1.5

### Patch Changes

- @pnpm/plugin-commands-installation@15.1.5
- @pnpm/fs.indexed-pkg-importer@6.0.2
- @pnpm/cli-utils@3.0.7
- @pnpm/directory-fetcher@8.0.2

## 5.1.4

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/cli-utils@3.0.6
  - @pnpm/directory-fetcher@8.0.2
  - @pnpm/plugin-commands-installation@15.1.4
  - @pnpm/fs.indexed-pkg-importer@6.0.1

## 5.1.3

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/cli-utils@3.0.5
  - @pnpm/plugin-commands-installation@15.1.3
  - @pnpm/directory-fetcher@8.0.1

## 5.1.2

### Patch Changes

- @pnpm/cli-utils@3.0.4
- @pnpm/plugin-commands-installation@15.1.2

## 5.1.1

### Patch Changes

- @pnpm/plugin-commands-installation@15.1.1

## 5.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/plugin-commands-installation@15.1.0
  - @pnpm/cli-utils@3.0.3
  - @pnpm/fs.indexed-pkg-importer@6.0.0
  - @pnpm/directory-fetcher@8.0.0

## 5.0.7

### Patch Changes

- @pnpm/plugin-commands-installation@15.0.7

## 5.0.6

### Patch Changes

- @pnpm/plugin-commands-installation@15.0.6

## 5.0.5

### Patch Changes

- @pnpm/plugin-commands-installation@15.0.5

## 5.0.4

### Patch Changes

- Updated dependencies [a80b539]
  - @pnpm/cli-utils@3.0.2
  - @pnpm/plugin-commands-installation@15.0.4

## 5.0.3

### Patch Changes

- @pnpm/plugin-commands-installation@15.0.3

## 5.0.2

### Patch Changes

- @pnpm/plugin-commands-installation@15.0.2

## 5.0.1

### Patch Changes

- @pnpm/cli-utils@3.0.1
- @pnpm/plugin-commands-installation@15.0.1

## 5.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [3477ee5]
- Updated dependencies [d4e13ca]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/plugin-commands-installation@15.0.0
  - @pnpm/common-cli-options-help@2.0.0
  - @pnpm/directory-fetcher@8.0.0
  - @pnpm/fs.indexed-pkg-importer@6.0.0
  - @pnpm/cli-utils@3.0.0
  - @pnpm/fs.is-empty-dir-or-nothing@2.0.0

## 4.0.20

### Patch Changes

- Updated dependencies [31054a63e]
- Updated dependencies [e2e08b98f]
- Updated dependencies [f43bdcf45]
- Updated dependencies [df9b16aa9]
  - @pnpm/plugin-commands-installation@14.2.0
  - @pnpm/fs.indexed-pkg-importer@5.0.13
  - @pnpm/directory-fetcher@7.0.11
  - @pnpm/cli-utils@2.1.9

## 4.0.19

### Patch Changes

- @pnpm/directory-fetcher@7.0.10
- @pnpm/plugin-commands-installation@14.1.3

## 4.0.18

### Patch Changes

- @pnpm/plugin-commands-installation@14.1.2
- @pnpm/cli-utils@2.1.8
- @pnpm/fs.indexed-pkg-importer@5.0.12
- @pnpm/directory-fetcher@7.0.9

## 4.0.17

### Patch Changes

- Updated dependencies [19be6b704]
  - @pnpm/fs.indexed-pkg-importer@5.0.12
  - @pnpm/plugin-commands-installation@14.1.1

## 4.0.16

### Patch Changes

- 693944b66: `pnpm deploy` should not touch the target directory if it already exists and isn't empty [#7351](https://github.com/pnpm/pnpm/issues/7351).
- Updated dependencies [064aeb681]
- Updated dependencies [693944b66]
  - @pnpm/plugin-commands-installation@14.1.0
  - @pnpm/fs.is-empty-dir-or-nothing@1.0.0
  - @pnpm/cli-utils@2.1.7

## 4.0.15

### Patch Changes

- Updated dependencies [619e9ed6f]
- Updated dependencies [4e71066dd]
- Updated dependencies [33313d2fd]
- Updated dependencies [4d34684f1]
  - @pnpm/plugin-commands-installation@14.0.15
  - @pnpm/common-cli-options-help@1.1.0
  - @pnpm/fs.indexed-pkg-importer@5.0.11
  - @pnpm/types@9.4.2
  - @pnpm/cli-utils@2.1.6
  - @pnpm/directory-fetcher@7.0.9

## 4.0.14

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/plugin-commands-installation@14.0.14
  - @pnpm/cli-utils@2.1.5
  - @pnpm/directory-fetcher@7.0.8
  - @pnpm/fs.indexed-pkg-importer@5.0.10

## 4.0.13

### Patch Changes

- Updated dependencies [418866ac0]
  - @pnpm/fs.indexed-pkg-importer@5.0.9
  - @pnpm/plugin-commands-installation@14.0.13

## 4.0.12

### Patch Changes

- @pnpm/plugin-commands-installation@14.0.12

## 4.0.11

### Patch Changes

- Updated dependencies [6558d1865]
  - @pnpm/plugin-commands-installation@14.0.11
  - @pnpm/cli-utils@2.1.4

## 4.0.10

### Patch Changes

- @pnpm/cli-utils@2.1.3
- @pnpm/plugin-commands-installation@14.0.10

## 4.0.9

### Patch Changes

- @pnpm/plugin-commands-installation@14.0.9

## 4.0.8

### Patch Changes

- @pnpm/plugin-commands-installation@14.0.8

## 4.0.7

### Patch Changes

- @pnpm/plugin-commands-installation@14.0.7

## 4.0.6

### Patch Changes

- @pnpm/fs.indexed-pkg-importer@5.0.8
- @pnpm/plugin-commands-installation@14.0.6
- @pnpm/directory-fetcher@7.0.7
- @pnpm/cli-utils@2.1.2

## 4.0.5

### Patch Changes

- @pnpm/plugin-commands-installation@14.0.5

## 4.0.4

### Patch Changes

- @pnpm/directory-fetcher@7.0.6
- @pnpm/plugin-commands-installation@14.0.4

## 4.0.3

### Patch Changes

- @pnpm/directory-fetcher@7.0.5
- @pnpm/plugin-commands-installation@14.0.3
- @pnpm/fs.indexed-pkg-importer@5.0.7
- @pnpm/cli-utils@2.1.1

## 4.0.2

### Patch Changes

- Updated dependencies [500363647]
  - @pnpm/directory-fetcher@7.0.4
  - @pnpm/plugin-commands-installation@14.0.2

## 4.0.1

### Patch Changes

- @pnpm/plugin-commands-installation@14.0.1

## 4.0.0

### Major Changes

- d6592964f: `rootProjectManifestDir` is a required field.

### Patch Changes

- Updated dependencies [43ce9e4a6]
- Updated dependencies [d6592964f]
- Updated dependencies [d6592964f]
  - @pnpm/types@9.4.0
  - @pnpm/cli-utils@2.1.0
  - @pnpm/plugin-commands-installation@14.0.0
  - @pnpm/fs.indexed-pkg-importer@5.0.6
  - @pnpm/directory-fetcher@7.0.3

## 3.1.15

### Patch Changes

- @pnpm/plugin-commands-installation@13.2.6

## 3.1.14

### Patch Changes

- Updated dependencies [2ca756fd2]
  - @pnpm/fs.indexed-pkg-importer@5.0.5
  - @pnpm/plugin-commands-installation@13.2.5

## 3.1.13

### Patch Changes

- Updated dependencies [bc83798d4]
- Updated dependencies [46dc34dcc]
- Updated dependencies [6dfbca86b]
  - @pnpm/plugin-commands-installation@13.2.4
  - @pnpm/fs.indexed-pkg-importer@5.0.4
  - @pnpm/cli-utils@2.0.24
  - @pnpm/directory-fetcher@7.0.2

## 3.1.12

### Patch Changes

- Updated dependencies [e19de6a59]
  - @pnpm/fs.indexed-pkg-importer@5.0.3
  - @pnpm/plugin-commands-installation@13.2.3

## 3.1.11

### Patch Changes

- Updated dependencies [6337dcdbc]
  - @pnpm/fs.indexed-pkg-importer@5.0.2
  - @pnpm/plugin-commands-installation@13.2.2

## 3.1.10

### Patch Changes

- @pnpm/cli-utils@2.0.23
- @pnpm/plugin-commands-installation@13.2.1

## 3.1.9

### Patch Changes

- Updated dependencies [ee6e0734e]
- Updated dependencies [12f45a83d]
- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/fs.indexed-pkg-importer@5.0.1
  - @pnpm/plugin-commands-installation@13.2.0
  - @pnpm/types@9.3.0
  - @pnpm/cli-utils@2.0.22
  - @pnpm/directory-fetcher@7.0.2

## 3.1.8

### Patch Changes

- @pnpm/plugin-commands-installation@13.1.8

## 3.1.7

### Patch Changes

- Updated dependencies [ba48fe0bc]
  - @pnpm/plugin-commands-installation@13.1.7
  - @pnpm/cli-utils@2.0.21

## 3.1.6

### Patch Changes

- @pnpm/plugin-commands-installation@13.1.6
- @pnpm/cli-utils@2.0.20

## 3.1.5

### Patch Changes

- Updated dependencies [9caa33d53]
  - @pnpm/fs.indexed-pkg-importer@5.0.0
  - @pnpm/plugin-commands-installation@13.1.5
  - @pnpm/cli-utils@2.0.19
  - @pnpm/directory-fetcher@7.0.1

## 3.1.4

### Patch Changes

- Updated dependencies [cb6e4212c]
  - @pnpm/fs.indexed-pkg-importer@4.1.1
  - @pnpm/plugin-commands-installation@13.1.4

## 3.1.3

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/fs.indexed-pkg-importer@4.1.0
  - @pnpm/plugin-commands-installation@13.1.3
  - @pnpm/cli-utils@2.0.18
  - @pnpm/directory-fetcher@7.0.0

## 3.1.2

### Patch Changes

- @pnpm/plugin-commands-installation@13.1.2
- @pnpm/fs.indexed-pkg-importer@4.0.1
- @pnpm/directory-fetcher@7.0.0

## 3.1.1

### Patch Changes

- d92070876: Reverting a change shipped in v8.7 that caused issues with the `pnpm deploy` command and "injected dependencies" [#6943](https://github.com/pnpm/pnpm/pull/6943).
- Updated dependencies [4a1a9431d]
- Updated dependencies [d92070876]
  - @pnpm/directory-fetcher@7.0.0
  - @pnpm/plugin-commands-installation@13.1.1
  - @pnpm/fs.indexed-pkg-importer@4.0.1
  - @pnpm/cli-utils@2.0.17

## 3.1.0

### Minor Changes

- d57e4de6d: Apply `publishConfig` for workspace packages on directory fetch. Enables a publishable ("exportable") `package.json` on deployment [#6693](https://github.com/pnpm/pnpm/issues/6693).

### Patch Changes

- Updated dependencies [ef3609049]
- Updated dependencies [e0474bc4c]
- Updated dependencies [d57e4de6d]
- Updated dependencies [f2009d175]
- Updated dependencies [bf21c9bf3]
- Updated dependencies [81e5ada3a]
  - @pnpm/plugin-commands-installation@13.1.0
  - @pnpm/directory-fetcher@6.1.0
  - @pnpm/fs.indexed-pkg-importer@4.0.0
  - @pnpm/cli-utils@2.0.16

## 3.0.31

### Patch Changes

- Updated dependencies [12b0f0976]
  - @pnpm/plugin-commands-installation@13.0.25
  - @pnpm/cli-utils@2.0.15

## 3.0.30

### Patch Changes

- 78d43a862: Always set `dedupe-peer-dependents` to `false`, when running installation during deploy [#6858](https://github.com/pnpm/pnpm/issues/6858).
- Updated dependencies [78d43a862]
  - @pnpm/plugin-commands-installation@13.0.24

## 3.0.29

### Patch Changes

- @pnpm/cli-utils@2.0.14
- @pnpm/plugin-commands-installation@13.0.23

## 3.0.28

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.22
- @pnpm/fs.indexed-pkg-importer@3.0.2
- @pnpm/directory-fetcher@6.0.4

## 3.0.27

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.21
- @pnpm/fs.indexed-pkg-importer@3.0.2
- @pnpm/directory-fetcher@6.0.4

## 3.0.26

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.20
- @pnpm/fs.indexed-pkg-importer@3.0.2
- @pnpm/directory-fetcher@6.0.4

## 3.0.25

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.19

## 3.0.24

### Patch Changes

- Updated dependencies [aa2ae8fe2]
- Updated dependencies [e958707b2]
  - @pnpm/types@9.2.0
  - @pnpm/fs.indexed-pkg-importer@3.0.2
  - @pnpm/cli-utils@2.0.13
  - @pnpm/plugin-commands-installation@13.0.18
  - @pnpm/directory-fetcher@6.0.4

## 3.0.23

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.17

## 3.0.22

### Patch Changes

- @pnpm/cli-utils@2.0.12
- @pnpm/directory-fetcher@6.0.3
- @pnpm/plugin-commands-installation@13.0.16

## 3.0.21

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.15
- @pnpm/fs.indexed-pkg-importer@3.0.1
- @pnpm/directory-fetcher@6.0.2

## 3.0.20

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.14

## 3.0.19

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.13

## 3.0.18

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.12

## 3.0.17

### Patch Changes

- Updated dependencies [0b830f947]
  - @pnpm/plugin-commands-installation@13.0.11
  - @pnpm/cli-utils@2.0.11
  - @pnpm/fs.indexed-pkg-importer@3.0.1
  - @pnpm/directory-fetcher@6.0.2

## 3.0.16

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.10
- @pnpm/error@5.0.2
- @pnpm/cli-utils@2.0.10
- @pnpm/fs.indexed-pkg-importer@3.0.1
- @pnpm/directory-fetcher@6.0.2

## 3.0.15

### Patch Changes

- Updated dependencies [d55b41a8b]
  - @pnpm/plugin-commands-installation@13.0.9
  - @pnpm/fs.indexed-pkg-importer@3.0.1
  - @pnpm/directory-fetcher@6.0.1

## 3.0.14

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [04a279881]
  - @pnpm/types@9.1.0
  - @pnpm/plugin-commands-installation@13.0.8
  - @pnpm/cli-utils@2.0.9
  - @pnpm/error@5.0.1
  - @pnpm/fs.indexed-pkg-importer@3.0.1
  - @pnpm/directory-fetcher@6.0.1

## 3.0.13

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.7

## 3.0.12

### Patch Changes

- Updated dependencies [ee429b300]
  - @pnpm/cli-utils@2.0.8
  - @pnpm/plugin-commands-installation@13.0.6

## 3.0.11

### Patch Changes

- Updated dependencies [d5c40b556]
  - @pnpm/plugin-commands-installation@13.0.5

## 3.0.10

### Patch Changes

- 1ffedcb8d: The deploy command should not ask for confirmation to purge the `node_modules` directory [#6510](https://github.com/pnpm/pnpm/issues/6510).
  - @pnpm/plugin-commands-installation@13.0.4

## 3.0.9

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.3
- @pnpm/cli-utils@2.0.7
- @pnpm/fs.indexed-pkg-importer@3.0.0
- @pnpm/directory-fetcher@6.0.0

## 3.0.8

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.2

## 3.0.7

### Patch Changes

- @pnpm/plugin-commands-installation@13.0.1

## 3.0.6

### Patch Changes

- Updated dependencies [8e7a86dd9]
- Updated dependencies [6706a7d17]
- Updated dependencies [6850bb135]
- Updated dependencies [71a3ee77b]
- Updated dependencies [8e7a86dd9]
  - @pnpm/plugin-commands-installation@13.0.0
  - @pnpm/cli-utils@2.0.6

## 3.0.5

### Patch Changes

- Updated dependencies [e440d784f]
  - @pnpm/plugin-commands-installation@12.1.2
  - @pnpm/cli-utils@2.0.5

## 3.0.4

### Patch Changes

- @pnpm/plugin-commands-installation@12.1.1
- @pnpm/cli-utils@2.0.4

## 3.0.3

### Patch Changes

- Updated dependencies [e2cb4b63d]
  - @pnpm/plugin-commands-installation@12.1.0
  - @pnpm/cli-utils@2.0.3

## 3.0.2

### Patch Changes

- @pnpm/plugin-commands-installation@12.0.2
- @pnpm/cli-utils@2.0.2

## 3.0.1

### Patch Changes

- Updated dependencies [51445f955]
  - @pnpm/plugin-commands-installation@12.0.1
  - @pnpm/cli-utils@2.0.1

## 3.0.0

### Major Changes

- 7a0ce1df0: When there's a `files` field in the `package.json`, only deploy those files that are listed in it.
  Use the same logic also when injecting packages. This behavior can be changed by setting the `deploy-all-files` setting to `true` [#5911](https://github.com/pnpm/pnpm/issues/5911).
- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [cae85dbb1]
- Updated dependencies [22ccf155e]
- Updated dependencies [7a0ce1df0]
- Updated dependencies [eceaa8b8b]
  - @pnpm/plugin-commands-installation@12.0.0
  - @pnpm/common-cli-options-help@1.0.0
  - @pnpm/directory-fetcher@6.0.0
  - @pnpm/fs.indexed-pkg-importer@3.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0
  - @pnpm/cli-utils@2.0.0

## 2.0.42

### Patch Changes

- @pnpm/plugin-commands-installation@11.5.7
- @pnpm/cli-utils@1.1.7

## 2.0.41

### Patch Changes

- @pnpm/cli-utils@1.1.6
- @pnpm/plugin-commands-installation@11.5.6

## 2.0.40

### Patch Changes

- Updated dependencies [955874422]
  - @pnpm/fs.indexed-pkg-importer@2.1.4
  - @pnpm/plugin-commands-installation@11.5.5
  - @pnpm/cli-utils@1.1.5
  - @pnpm/directory-fetcher@5.1.6

## 2.0.39

### Patch Changes

- @pnpm/plugin-commands-installation@11.5.4
- @pnpm/cli-utils@1.1.4

## 2.0.38

### Patch Changes

- Updated dependencies [690bead26]
  - @pnpm/plugin-commands-installation@11.5.3
  - @pnpm/cli-utils@1.1.3

## 2.0.37

### Patch Changes

- Updated dependencies [7d64d757b]
  - @pnpm/cli-utils@1.1.2
  - @pnpm/plugin-commands-installation@11.5.2

## 2.0.36

### Patch Changes

- @pnpm/plugin-commands-installation@11.5.1
- @pnpm/cli-utils@1.1.1

## 2.0.35

### Patch Changes

- Updated dependencies [0377d9367]
  - @pnpm/plugin-commands-installation@11.5.0
  - @pnpm/cli-utils@1.1.0

## 2.0.34

### Patch Changes

- @pnpm/plugin-commands-installation@11.4.6
- @pnpm/cli-utils@1.0.34

## 2.0.33

### Patch Changes

- @pnpm/directory-fetcher@5.1.5
- @pnpm/plugin-commands-installation@11.4.5
- @pnpm/fs.indexed-pkg-importer@2.1.3
- @pnpm/cli-utils@1.0.33

## 2.0.32

### Patch Changes

- Updated dependencies [308eb2c9b]
  - @pnpm/plugin-commands-installation@11.4.4
  - @pnpm/cli-utils@1.0.32

## 2.0.31

### Patch Changes

- Updated dependencies [6348f5931]
  - @pnpm/plugin-commands-installation@11.4.3
  - @pnpm/cli-utils@1.0.31

## 2.0.30

### Patch Changes

- Updated dependencies [78d4cf1f7]
  - @pnpm/fs.indexed-pkg-importer@2.1.2
  - @pnpm/plugin-commands-installation@11.4.2
  - @pnpm/cli-utils@1.0.30

## 2.0.29

### Patch Changes

- @pnpm/plugin-commands-installation@11.4.1
- @pnpm/cli-utils@1.0.29

## 2.0.28

### Patch Changes

- Updated dependencies [e8f6ab683]
  - @pnpm/plugin-commands-installation@11.4.0
  - @pnpm/cli-utils@1.0.28

## 2.0.27

### Patch Changes

- Updated dependencies [4655dd41e]
  - @pnpm/plugin-commands-installation@11.3.5
  - @pnpm/fs.indexed-pkg-importer@2.1.1
  - @pnpm/cli-utils@1.0.27
  - @pnpm/directory-fetcher@5.1.4

## 2.0.26

### Patch Changes

- @pnpm/plugin-commands-installation@11.3.4
- @pnpm/cli-utils@1.0.26

## 2.0.25

### Patch Changes

- @pnpm/plugin-commands-installation@11.3.3
- @pnpm/cli-utils@1.0.25
- @pnpm/fs.indexed-pkg-importer@2.1.1
- @pnpm/directory-fetcher@5.1.4

## 2.0.24

### Patch Changes

- @pnpm/plugin-commands-installation@11.3.2
- @pnpm/cli-utils@1.0.24

## 2.0.23

### Patch Changes

- @pnpm/plugin-commands-installation@11.3.1
- @pnpm/cli-utils@1.0.23

## 2.0.22

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/plugin-commands-installation@11.3.0
  - @pnpm/fs.indexed-pkg-importer@2.1.1
  - @pnpm/error@4.0.1
  - @pnpm/cli-utils@1.0.22
  - @pnpm/directory-fetcher@5.1.4

## 2.0.21

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/plugin-commands-installation@11.2.0
  - @pnpm/cli-utils@1.0.21

## 2.0.20

### Patch Changes

- Updated dependencies [08ceaf3fc]
  - @pnpm/plugin-commands-installation@11.1.7
  - @pnpm/cli-utils@1.0.20

## 2.0.19

### Patch Changes

- Updated dependencies [d71dbf230]
  - @pnpm/plugin-commands-installation@11.1.6
  - @pnpm/cli-utils@1.0.19

## 2.0.18

### Patch Changes

- @pnpm/plugin-commands-installation@11.1.5
- @pnpm/cli-utils@1.0.18

## 2.0.17

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/fs.indexed-pkg-importer@2.1.0
  - @pnpm/plugin-commands-installation@11.1.4
  - @pnpm/cli-utils@1.0.17
  - @pnpm/directory-fetcher@5.1.3

## 2.0.16

### Patch Changes

- @pnpm/plugin-commands-installation@11.1.3
- @pnpm/cli-utils@1.0.16

## 2.0.15

### Patch Changes

- Updated dependencies [49f6c917f]
  - @pnpm/plugin-commands-installation@11.1.2
  - @pnpm/cli-utils@1.0.15

## 2.0.14

### Patch Changes

- @pnpm/cli-utils@1.0.14
- @pnpm/plugin-commands-installation@11.1.1

## 2.0.13

### Patch Changes

- Updated dependencies [4097af6b5]
  - @pnpm/plugin-commands-installation@11.1.0
  - @pnpm/cli-utils@1.0.13
  - @pnpm/directory-fetcher@5.1.2
  - @pnpm/fs.indexed-pkg-importer@2.0.2

## 2.0.12

### Patch Changes

- @pnpm/plugin-commands-installation@11.0.12
- @pnpm/cli-utils@1.0.12

## 2.0.11

### Patch Changes

- @pnpm/plugin-commands-installation@11.0.11
- @pnpm/cli-utils@1.0.11

## 2.0.10

### Patch Changes

- Updated dependencies [868f2fb16]
  - @pnpm/plugin-commands-installation@11.0.10
  - @pnpm/cli-utils@1.0.10
  - @pnpm/directory-fetcher@5.1.1

## 2.0.9

### Patch Changes

- Updated dependencies [969f8a002]
  - @pnpm/plugin-commands-installation@11.0.9
  - @pnpm/cli-utils@1.0.9

## 2.0.8

### Patch Changes

- @pnpm/plugin-commands-installation@11.0.8
- @pnpm/cli-utils@1.0.8

## 2.0.7

### Patch Changes

- Updated dependencies [eacff33e4]
  - @pnpm/directory-fetcher@5.1.0
  - @pnpm/plugin-commands-installation@11.0.7
  - @pnpm/cli-utils@1.0.7

## 2.0.6

### Patch Changes

- Updated dependencies [3dab7f83c]
  - @pnpm/plugin-commands-installation@11.0.6
  - @pnpm/cli-utils@1.0.6

## 2.0.5

### Patch Changes

- Updated dependencies [6710d9dd9]
- Updated dependencies [702e847c1]
- Updated dependencies [6710d9dd9]
  - @pnpm/directory-fetcher@5.0.0
  - @pnpm/types@8.9.0
  - @pnpm/cli-utils@1.0.5
  - @pnpm/plugin-commands-installation@11.0.5
  - @pnpm/fs.indexed-pkg-importer@2.0.2

## 2.0.4

### Patch Changes

- Updated dependencies [0da2f0412]
  - @pnpm/plugin-commands-installation@11.0.4
  - @pnpm/cli-utils@1.0.4

## 2.0.3

### Patch Changes

- @pnpm/cli-utils@1.0.3
- @pnpm/plugin-commands-installation@11.0.3

## 2.0.2

### Patch Changes

- @pnpm/cli-utils@1.0.2
- @pnpm/plugin-commands-installation@11.0.2

## 2.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/cli-utils@1.0.1
  - @pnpm/plugin-commands-installation@11.0.1
  - @pnpm/fs.indexed-pkg-importer@2.0.1
  - @pnpm/directory-fetcher@4.0.1

## 2.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [645384bfd]
- Updated dependencies [f884689e0]
- Updated dependencies [e35988d1f]
  - @pnpm/directory-fetcher@4.0.0
  - @pnpm/error@4.0.0
  - @pnpm/plugin-commands-installation@11.0.0
  - @pnpm/cli-utils@1.0.0
  - @pnpm/fs.indexed-pkg-importer@2.0.0

## 1.1.13

### Patch Changes

- Updated dependencies [96b507b73]
  - @pnpm/plugin-commands-installation@10.8.4
  - @pnpm/cli-utils@0.7.43
  - @pnpm/directory-fetcher@3.1.5

## 1.1.12

### Patch Changes

- Updated dependencies [3277188eb]
  - @pnpm/plugin-commands-installation@10.8.3
  - @pnpm/fs.indexed-pkg-importer@1.1.4
  - @pnpm/cli-utils@0.7.42

## 1.1.11

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/cli-utils@0.7.41
  - @pnpm/plugin-commands-installation@10.8.2
  - @pnpm/directory-fetcher@3.1.4

## 1.1.10

### Patch Changes

- Updated dependencies [536b16856]
  - @pnpm/plugin-commands-installation@10.8.1

## 1.1.9

### Patch Changes

- 51566e34b: Hooks should be applied on `pnpm deploy` [#5306](https://github.com/pnpm/pnpm/issues/5306).
- Updated dependencies [abb41a626]
- Updated dependencies [51566e34b]
- Updated dependencies [5beb4e26b]
- Updated dependencies [d665f3ff7]
  - @pnpm/plugin-commands-installation@10.8.0
  - @pnpm/types@8.7.0
  - @pnpm/cli-utils@0.7.40
  - @pnpm/fs.indexed-pkg-importer@1.1.3
  - @pnpm/directory-fetcher@3.1.3

## 1.1.8

### Patch Changes

- Updated dependencies [56aeba4ba]
- Updated dependencies [56aeba4ba]
  - @pnpm/plugin-commands-installation@10.7.2
  - @pnpm/cli-utils@0.7.39

## 1.1.7

### Patch Changes

- @pnpm/plugin-commands-installation@10.7.1
- @pnpm/cli-utils@0.7.38

## 1.1.6

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/plugin-commands-installation@10.7.0
  - @pnpm/types@8.6.0
  - @pnpm/cli-utils@0.7.37
  - @pnpm/fs.indexed-pkg-importer@1.1.2
  - @pnpm/directory-fetcher@3.1.2

## 1.1.5

### Patch Changes

- @pnpm/plugin-commands-installation@10.6.5
- @pnpm/cli-utils@0.7.36

## 1.1.4

### Patch Changes

- @pnpm/plugin-commands-installation@10.6.4
- @pnpm/cli-utils@0.7.35

## 1.1.3

### Patch Changes

- @pnpm/plugin-commands-installation@10.6.3
- @pnpm/cli-utils@0.7.34

## 1.1.2

### Patch Changes

- @pnpm/plugin-commands-installation@10.6.2
- @pnpm/cli-utils@0.7.33

## 1.1.1

### Patch Changes

- @pnpm/plugin-commands-installation@10.6.1
- @pnpm/cli-utils@0.7.32

## 1.1.0

### Minor Changes

- 2aa22e4b1: Set `NODE_PATH` when `preferSymlinkedExecutables` is enabled.

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/plugin-commands-installation@10.6.0
  - @pnpm/cli-utils@0.7.31

## 1.0.19

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.8
- @pnpm/cli-utils@0.7.30

## 1.0.18

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.7
- @pnpm/fs.indexed-pkg-importer@1.1.1
- @pnpm/cli-utils@0.7.29
- @pnpm/directory-fetcher@3.1.1

## 1.0.17

### Patch Changes

- Updated dependencies [9faf0221d]
- Updated dependencies [07bc24ad1]
  - @pnpm/plugin-commands-installation@10.5.6
  - @pnpm/directory-fetcher@3.1.1
  - @pnpm/cli-utils@0.7.28
  - @pnpm/fs.indexed-pkg-importer@1.1.1

## 1.0.16

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/directory-fetcher@3.1.0
  - @pnpm/fs.indexed-pkg-importer@1.1.1
  - @pnpm/plugin-commands-installation@10.5.5
  - @pnpm/cli-utils@0.7.27

## 1.0.15

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.4
- @pnpm/fs.indexed-pkg-importer@1.1.0
- @pnpm/directory-fetcher@3.0.10

## 1.0.14

### Patch Changes

- Updated dependencies [39c040127]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/directory-fetcher@3.0.10
  - @pnpm/plugin-commands-installation@10.5.3
  - @pnpm/fs.indexed-pkg-importer@1.1.0
  - @pnpm/cli-utils@0.7.26

## 1.0.13

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/cli-utils@0.7.25
  - @pnpm/plugin-commands-installation@10.5.2
  - @pnpm/fs.indexed-pkg-importer@1.0.1
  - @pnpm/directory-fetcher@3.0.9

## 1.0.12

### Patch Changes

- @pnpm/plugin-commands-installation@10.5.1

## 1.0.11

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/plugin-commands-installation@10.5.0
  - @pnpm/cli-utils@0.7.24

## 1.0.10

### Patch Changes

- c7519ad6a: **pnpm deploy**: accept absolute paths and use cwd instead of workspaceDir for deploy target directory [#4980](https://github.com/pnpm/pnpm/issues/4980).
  - @pnpm/plugin-commands-installation@10.4.2
  - @pnpm/cli-utils@0.7.23
  - @pnpm/fs.indexed-pkg-importer@1.0.0
  - @pnpm/directory-fetcher@3.0.8

## 1.0.9

### Patch Changes

- 107d01109: `pnpm deploy` should inject local dependencies of all types (dependencies, optionalDependencies, devDependencies) [#5078](https://github.com/pnpm/pnpm/issues/5078).
  - @pnpm/cli-utils@0.7.22
  - @pnpm/directory-fetcher@3.0.8
  - @pnpm/plugin-commands-installation@10.4.1

## 1.0.8

### Patch Changes

- 0569f1022: `pnpm deploy` should not modify the lockfile [#5071](https://github.com/pnpm/pnpm/issues/5071)
- 0569f1022: `pnpm deploy` should not fail in CI [#5071](https://github.com/pnpm/pnpm/issues/5071)
- Updated dependencies [0569f1022]
  - @pnpm/plugin-commands-installation@10.4.0
  - @pnpm/cli-utils@0.7.21

## 1.0.7

### Patch Changes

- 31e73ba77: `pnpm deploy` should include all dependencies by default [#5035](https://github.com/pnpm/pnpm/issues/5035).
- Updated dependencies [406656f80]
  - @pnpm/plugin-commands-installation@10.3.10
  - @pnpm/cli-utils@0.7.20

## 1.0.6

### Patch Changes

- @pnpm/plugin-commands-installation@10.3.9
- @pnpm/cli-utils@0.7.19

## 1.0.5

### Patch Changes

- @pnpm/plugin-commands-installation@10.3.8

## 1.0.4

### Patch Changes

- @pnpm/cli-utils@0.7.18
- @pnpm/plugin-commands-installation@10.3.7

## 1.0.3

### Patch Changes

- Updated dependencies [b55b3782d]
  - @pnpm/plugin-commands-installation@10.3.6

## 1.0.2

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/cli-utils@0.7.17
  - @pnpm/directory-fetcher@3.0.7
  - @pnpm/plugin-commands-installation@10.3.5

## 1.0.1

### Patch Changes

- f4248b514: Changes deployment directories to be created recursively
  - @pnpm/plugin-commands-installation@10.3.4

## 1.0.0

### Major Changes

- 7922d6314: A new experimental command added: `pnpm deploy`. The deploy command takes copies a project from a workspace and installs all of its production dependencies (even if some of those dependencies are other projects from the workspace).

  For example, the new command will deploy the project named `foo` to the `dist` directory in the root of the workspace:

  ```
  pnpm --filter=foo deploy dist
  ```

### Patch Changes

- Updated dependencies [7922d6314]
  - @pnpm/fs.indexed-pkg-importer@1.0.0
  - @pnpm/plugin-commands-installation@10.3.3
