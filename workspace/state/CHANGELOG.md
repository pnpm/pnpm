# @pnpm/workspace.state

## 1001.1.2

### Patch Changes

- Updated dependencies [b562deb]
- Updated dependencies [f3ffaed]
- Updated dependencies [c96eb2b]
  - @pnpm/types@1000.1.1
  - @pnpm/config@1002.2.0

## 1001.1.1

### Patch Changes

- @pnpm/config@1002.1.2

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
  - @pnpm/types@1000.1.0
  - @pnpm/config@1002.1.1

## 1001.0.2

### Patch Changes

- Updated dependencies [f90a94b]
- Updated dependencies [f891288]
  - @pnpm/config@1002.1.0

## 1001.0.1

### Patch Changes

- Updated dependencies [878ea8c]
  - @pnpm/config@1002.0.0

## 1001.0.0

### Major Changes

- d47c426: On repeat install perform a fast check if `node_modules` is up to date [#8838](https://github.com/pnpm/pnpm/pull/8838).

### Patch Changes

- Updated dependencies [ac5b9d8]
- Updated dependencies [6483b64]
  - @pnpm/config@1001.0.0

## 1.0.0

### Major Changes

- 19d5b51: Initial Release
