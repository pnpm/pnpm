# @pnpm/deps.status

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
