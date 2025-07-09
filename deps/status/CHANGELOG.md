# @pnpm/deps.status

## 1003.0.1

### Patch Changes

- @pnpm/workspace.find-packages@1000.0.29

## 1003.0.0

### Major Changes

- cf630a8: Added the possibility to load multiple pnpmfiles. The `pnpmfile` setting can now accept a list of pnpmfile locations [#9702](https://github.com/pnpm/pnpm/pull/9702).

### Patch Changes

- Updated dependencies [623da6f]
- Updated dependencies [cf630a8]
  - @pnpm/config@1004.1.0
  - @pnpm/workspace.state@1002.0.0
  - @pnpm/workspace.find-packages@1000.0.28
  - @pnpm/lockfile.settings-checker@1001.0.10
  - @pnpm/lockfile.verification@1001.2.2
  - @pnpm/lockfile.fs@1001.1.15
  - @pnpm/get-context@1001.1.2

## 1002.1.5

### Patch Changes

- @pnpm/lockfile.fs@1001.1.14
- @pnpm/lockfile.verification@1001.2.1
- @pnpm/get-context@1001.1.1
- @pnpm/workspace.find-packages@1000.0.27

## 1002.1.4

### Patch Changes

- b0ead51: Read the current lockfile from `node_modules/.pnpm/lock.yaml`, when the project uses a global virtual store.
- Updated dependencies [2721291]
- Updated dependencies [6acf819]
- Updated dependencies [86e0016]
- Updated dependencies [b217bbb]
- Updated dependencies [b0ead51]
- Updated dependencies [c8341cc]
- Updated dependencies [b0ead51]
- Updated dependencies [b0ead51]
- Updated dependencies [046af72]
  - @pnpm/resolver-base@1004.0.0
  - @pnpm/lockfile.verification@1001.2.0
  - @pnpm/get-context@1001.1.0
  - @pnpm/config@1004.0.0
  - @pnpm/workspace.read-manifest@1000.2.0
  - @pnpm/crypto.object-hasher@1000.1.0
  - @pnpm/workspace.state@1001.1.22
  - @pnpm/workspace.find-packages@1000.0.26
  - @pnpm/pnpmfile@1001.2.3
  - @pnpm/lockfile.fs@1001.1.13
  - @pnpm/lockfile.settings-checker@1001.0.9

## 1002.1.3

### Patch Changes

- Updated dependencies [8d175c0]
  - @pnpm/config@1003.1.1
  - @pnpm/workspace.state@1001.1.21
  - @pnpm/pnpmfile@1001.2.2
  - @pnpm/workspace.find-packages@1000.0.25
  - @pnpm/lockfile.settings-checker@1001.0.9
  - @pnpm/lockfile.verification@1001.1.7

## 1002.1.2

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [b282bd1]
- Updated dependencies [fdb1d98]
- Updated dependencies [e4af08c]
- Updated dependencies [09cf46f]
- Updated dependencies [36d1448]
- Updated dependencies [9362b5f]
- Updated dependencies [c00360b]
- Updated dependencies [5ec7255]
- Updated dependencies [6cf010c]
  - @pnpm/config@1003.1.0
  - @pnpm/get-context@1001.0.14
  - @pnpm/workspace.find-packages@1000.0.24
  - @pnpm/lockfile.verification@1001.1.7
  - @pnpm/workspace.state@1001.1.20
  - @pnpm/pnpmfile@1001.2.1
  - @pnpm/lockfile.fs@1001.1.12
  - @pnpm/types@1000.6.0
  - @pnpm/resolver-base@1003.0.1
  - @pnpm/workspace.read-manifest@1000.1.5
  - @pnpm/lockfile.settings-checker@1001.0.9

## 1002.1.1

### Patch Changes

- Updated dependencies [e5c58f0]
  - @pnpm/pnpmfile@1001.2.0
  - @pnpm/config@1003.0.1
  - @pnpm/workspace.find-packages@1000.0.23
  - @pnpm/workspace.state@1001.1.19

## 1002.1.0

### Minor Changes

- 3cf337b: Fix a false negative in `verify-deps-before-run` when `node-linker` is `hoisted` and there is a workspace package without dependencies and `node_modules` directory [#9424](https://github.com/pnpm/pnpm/issues/9424).
- 3cf337b: Explicitly drop `verify-deps-before-run` support for `node-linker=pnp`. Combining `verify-deps-before-run` and `node-linker=pnp` will now print a warning.

### Patch Changes

- Updated dependencies [56bb69b]
- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/config@1003.0.0
  - @pnpm/resolver-base@1003.0.0
  - @pnpm/parse-overrides@1001.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/types@1000.5.0
  - @pnpm/workspace.state@1001.1.18
  - @pnpm/pnpmfile@1001.1.2
  - @pnpm/lockfile.verification@1001.1.6
  - @pnpm/get-context@1001.0.13
  - @pnpm/lockfile.settings-checker@1001.0.8
  - @pnpm/lockfile.fs@1001.1.11
  - @pnpm/workspace.find-packages@1000.0.22
  - @pnpm/workspace.read-manifest@1000.1.4

## 1002.0.11

### Patch Changes

- Updated dependencies [81f441c]
  - @pnpm/resolver-base@1002.0.0
  - @pnpm/lockfile.verification@1001.1.5
  - @pnpm/get-context@1001.0.12
  - @pnpm/pnpmfile@1001.1.1
  - @pnpm/lockfile.fs@1001.1.10
  - @pnpm/workspace.find-packages@1000.0.21
  - @pnpm/config@1002.7.2
  - @pnpm/workspace.state@1001.1.17
  - @pnpm/lockfile.settings-checker@1001.0.7

## 1002.0.10

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [72cff38]
- Updated dependencies [5679712]
- Updated dependencies [01f2bcf]
- Updated dependencies [1413c25]
  - @pnpm/types@1000.4.0
  - @pnpm/resolver-base@1001.0.0
  - @pnpm/config@1002.7.1
  - @pnpm/pnpmfile@1001.1.0
  - @pnpm/lockfile.fs@1001.1.9
  - @pnpm/lockfile.verification@1001.1.4
  - @pnpm/get-context@1001.0.11
  - @pnpm/workspace.find-packages@1000.0.20
  - @pnpm/workspace.read-manifest@1000.1.3
  - @pnpm/workspace.state@1001.1.16
  - @pnpm/lockfile.settings-checker@1001.0.7

## 1002.0.9

### Patch Changes

- Updated dependencies [e57f1df]
  - @pnpm/config@1002.7.0
  - @pnpm/workspace.state@1001.1.15
  - @pnpm/workspace.find-packages@1000.0.19

## 1002.0.8

### Patch Changes

- Updated dependencies [9bcca9f]
- Updated dependencies [5b35dff]
- Updated dependencies [9bcca9f]
- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/config@1002.6.0
  - @pnpm/types@1000.3.0
  - @pnpm/workspace.state@1001.1.14
  - @pnpm/pnpmfile@1001.0.9
  - @pnpm/lockfile.fs@1001.1.8
  - @pnpm/lockfile.verification@1001.1.3
  - @pnpm/get-context@1001.0.10
  - @pnpm/resolver-base@1000.2.1
  - @pnpm/workspace.find-packages@1000.0.18
  - @pnpm/workspace.read-manifest@1000.1.2
  - @pnpm/lockfile.settings-checker@1001.0.6

## 1002.0.7

### Patch Changes

- Updated dependencies [936430a]
- Updated dependencies [3d52365]
  - @pnpm/config@1002.5.4
  - @pnpm/resolver-base@1000.2.0
  - @pnpm/get-context@1001.0.9
  - @pnpm/workspace.state@1001.1.13
  - @pnpm/lockfile.verification@1001.1.2
  - @pnpm/workspace.find-packages@1000.0.17
  - @pnpm/pnpmfile@1001.0.8
  - @pnpm/lockfile.fs@1001.1.7
  - @pnpm/lockfile.settings-checker@1001.0.5

## 1002.0.6

### Patch Changes

- Updated dependencies [9904675]
  - @pnpm/workspace.state@1001.1.12

## 1002.0.5

### Patch Changes

- Updated dependencies [6e4459c]
  - @pnpm/config@1002.5.3
  - @pnpm/workspace.state@1001.1.11
  - @pnpm/workspace.find-packages@1000.0.16

## 1002.0.4

### Patch Changes

- @pnpm/workspace.find-packages@1000.0.15
- @pnpm/pnpmfile@1001.0.7
- @pnpm/lockfile.settings-checker@1001.0.5
- @pnpm/lockfile.verification@1001.1.1
- @pnpm/config@1002.5.2
- @pnpm/lockfile.fs@1001.1.6
- @pnpm/workspace.state@1001.1.10
- @pnpm/get-context@1001.0.8

## 1002.0.3

### Patch Changes

- Updated dependencies [c3aa4d8]
  - @pnpm/config@1002.5.1
  - @pnpm/workspace.state@1001.1.9
  - @pnpm/workspace.find-packages@1000.0.14

## 1002.0.2

### Patch Changes

- Updated dependencies [daf47e9]
- Updated dependencies [a5e4965]
- Updated dependencies [d965748]
  - @pnpm/lockfile.verification@1001.1.0
  - @pnpm/types@1000.2.1
  - @pnpm/config@1002.5.0
  - @pnpm/workspace.find-packages@1000.0.13
  - @pnpm/pnpmfile@1001.0.6
  - @pnpm/lockfile.settings-checker@1001.0.4
  - @pnpm/lockfile.fs@1001.1.5
  - @pnpm/get-context@1001.0.7
  - @pnpm/resolver-base@1000.1.4
  - @pnpm/workspace.read-manifest@1000.1.1
  - @pnpm/workspace.state@1001.1.8

## 1002.0.1

### Patch Changes

- Updated dependencies [1c2eb8c]
  - @pnpm/config@1002.4.1
  - @pnpm/workspace.state@1001.1.7
  - @pnpm/workspace.find-packages@1000.0.12

## 1002.0.0

### Major Changes

- 8fcc221: Read `configDependencies` from `options`.

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [e32b1a2]
- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
  - @pnpm/config@1002.4.0
  - @pnpm/types@1000.2.0
  - @pnpm/workspace.read-manifest@1000.1.0
  - @pnpm/workspace.state@1001.1.6
  - @pnpm/pnpmfile@1001.0.5
  - @pnpm/lockfile.fs@1001.1.4
  - @pnpm/lockfile.verification@1001.0.6
  - @pnpm/get-context@1001.0.6
  - @pnpm/resolver-base@1000.1.3
  - @pnpm/workspace.find-packages@1000.0.11
  - @pnpm/lockfile.settings-checker@1001.0.3

## 1001.2.2

### Patch Changes

- Updated dependencies [fee898f]
  - @pnpm/config@1002.3.1
  - @pnpm/workspace.state@1001.1.5
  - @pnpm/lockfile.fs@1001.1.3
  - @pnpm/workspace.find-packages@1000.0.10
  - @pnpm/get-context@1001.0.5
  - @pnpm/lockfile.verification@1001.0.5

## 1001.2.1

### Patch Changes

- @pnpm/workspace.find-packages@1000.0.9

## 1001.2.0

### Minor Changes

- 265946b: Fix a false negative of `verify-deps-before-run` after `pnpm install --production|--no-optional` [#9019](https://github.com/pnpm/pnpm/issues/9019).

### Patch Changes

- Updated dependencies [f6006f2]
- Updated dependencies [3717340]
  - @pnpm/config@1002.3.0
  - @pnpm/crypto.object-hasher@1000.0.1
  - @pnpm/workspace.state@1001.1.4
  - @pnpm/workspace.find-packages@1000.0.8

## 1001.1.3

### Patch Changes

- @pnpm/config@1002.2.1
- @pnpm/workspace.find-packages@1000.0.7
- @pnpm/workspace.state@1001.1.3

## 1001.1.2

### Patch Changes

- 5c8654f: Make sure that the deletion of a `node_modules` in a sub-project of a monorepo is detected as out-of-date [#8959](https://github.com/pnpm/pnpm/issues/8959).
- Updated dependencies [9a44e6c]
- Updated dependencies [b562deb]
- Updated dependencies [f3ffaed]
- Updated dependencies [c96eb2b]
  - @pnpm/constants@1001.1.0
  - @pnpm/workspace.find-packages@1000.0.6
  - @pnpm/types@1000.1.1
  - @pnpm/config@1002.2.0
  - @pnpm/lockfile.fs@1001.1.2
  - @pnpm/lockfile.verification@1001.0.4
  - @pnpm/error@1000.0.2
  - @pnpm/get-context@1001.0.4
  - @pnpm/workspace.read-manifest@1000.0.2
  - @pnpm/pnpmfile@1001.0.4
  - @pnpm/resolver-base@1000.1.2
  - @pnpm/workspace.state@1001.1.2
  - @pnpm/parse-overrides@1000.0.2
  - @pnpm/lockfile.settings-checker@1001.0.2

## 1001.1.1

### Patch Changes

- @pnpm/config@1002.1.2
- @pnpm/pnpmfile@1001.0.3
- @pnpm/workspace.find-packages@1000.0.5
- @pnpm/workspace.state@1001.1.1
- @pnpm/lockfile.settings-checker@1001.0.1
- @pnpm/lockfile.verification@1001.0.3

## 1001.1.0

### Minor Changes

- 9591a18: Added support for a new type of dependencies called "configurational dependencies". These dependencies are installed before all the other types of dependencies (before "dependencies", "devDependencies", "optionalDependencies").

  Configurational dependencies cannot have dependencies of their own or lifecycle scripts. They should be added using exact version and the integrity checksum. Example:

  ```json
  {
    "pnpm": {
      "configDependencies": {
        "my-configs": "1.0.0+sha512-30iZtAPgz+LTIYoeivqYo853f02jBYSd5uGnGpkFV0M3xOt9aN73erkgYAmZU43x4VfqcnLxW9Kpg3R5LC4YYw=="
      }
    }
  }
  ```

  Related RFC: [#8](https://github.com/pnpm/rfcs/pull/8).
  Related PR: [#8915](https://github.com/pnpm/pnpm/pull/8915).

### Patch Changes

- Updated dependencies [9591a18]
- Updated dependencies [1f5169f]
  - @pnpm/workspace.state@1001.1.0
  - @pnpm/types@1000.1.0
  - @pnpm/config@1002.1.1
  - @pnpm/pnpmfile@1001.0.2
  - @pnpm/lockfile.fs@1001.1.1
  - @pnpm/lockfile.verification@1001.0.3
  - @pnpm/get-context@1001.0.3
  - @pnpm/resolver-base@1000.1.1
  - @pnpm/workspace.find-packages@1000.0.4
  - @pnpm/lockfile.settings-checker@1001.0.1

## 1001.0.3

### Patch Changes

- Updated dependencies [f90a94b]
- Updated dependencies [f891288]
  - @pnpm/config@1002.1.0
  - @pnpm/workspace.state@1001.0.2
  - @pnpm/workspace.find-packages@1000.0.3

## 1001.0.2

### Patch Changes

- Updated dependencies [878ea8c]
  - @pnpm/config@1002.0.0
  - @pnpm/pnpmfile@1001.0.1
  - @pnpm/get-context@1001.0.2
  - @pnpm/workspace.state@1001.0.1
  - @pnpm/lockfile.verification@1001.0.2
  - @pnpm/workspace.find-packages@1000.0.2
  - @pnpm/lockfile.settings-checker@1001.0.0

## 1001.0.1

### Patch Changes

- Updated dependencies [3f0e4f0]
  - @pnpm/lockfile.fs@1001.1.0
  - @pnpm/get-context@1001.0.1
  - @pnpm/lockfile.verification@1001.0.1

## 1001.0.0

### Major Changes

- d47c426: On repeat install perform a fast check if `node_modules` is up to date [#8838](https://github.com/pnpm/pnpm/pull/8838).
- a76da0c: Removed lockfile conversion from v6 to v9. If you need to convert lockfile v6 to v9, use pnpm CLI v9.

### Minor Changes

- 6483b64: A new setting, `inject-workspace-packages`, has been added to allow hard-linking all local workspace dependencies instead of symlinking them. Previously, this behavior was achievable via the [`dependenciesMeta[].injected`](https://pnpm.io/package_json#dependenciesmetainjected) setting, which remains supported [#8836](https://github.com/pnpm/pnpm/pull/8836).

### Patch Changes

- Updated dependencies [ac5b9d8]
- Updated dependencies [d2e83b0]
- Updated dependencies [6483b64]
- Updated dependencies [d47c426]
- Updated dependencies [a76da0c]
  - @pnpm/config@1001.0.0
  - @pnpm/constants@1001.0.0
  - @pnpm/lockfile.settings-checker@1001.0.0
  - @pnpm/resolver-base@1000.1.0
  - @pnpm/pnpmfile@1001.0.0
  - @pnpm/workspace.state@1001.0.0
  - @pnpm/get-context@1001.0.0
  - @pnpm/lockfile.verification@1001.0.0
  - @pnpm/lockfile.fs@1001.0.0
  - @pnpm/error@1000.0.1
  - @pnpm/workspace.read-manifest@1000.0.1
  - @pnpm/workspace.find-packages@1000.0.1
  - @pnpm/parse-overrides@1000.0.1

## 1.0.0

### Major Changes

- 19d5b51: Initial Release

### Patch Changes

- Updated dependencies [19d5b51]
- Updated dependencies [8108680]
- Updated dependencies [477e0c1]
- Updated dependencies [19d5b51]
- Updated dependencies [dfcf034]
- Updated dependencies [19d5b51]
- Updated dependencies [592e2ef]
- Updated dependencies [9ea8fa4]
- Updated dependencies [9ea8fa4]
- Updated dependencies [19d5b51]
- Updated dependencies [9ea8fa4]
- Updated dependencies [1dbc56a]
- Updated dependencies [9ea8fa4]
- Updated dependencies [501c152]
- Updated dependencies [9ea8fa4]
- Updated dependencies [e9985b6]
- Updated dependencies [c4f5231]
  - @pnpm/constants@10.0.0
  - @pnpm/config@22.0.0
  - @pnpm/workspace.state@1.0.0
  - @pnpm/lockfile.verification@1.1.0
  - @pnpm/get-context@13.0.0
  - @pnpm/crypto.object-hasher@3.0.0
  - @pnpm/lockfile.fs@1.0.6
  - @pnpm/error@6.0.3
  - @pnpm/workspace.read-manifest@2.2.2
  - @pnpm/lockfile.settings-checker@1.0.2
  - @pnpm/parse-overrides@5.1.2
  - @pnpm/workspace.find-packages@4.0.13
