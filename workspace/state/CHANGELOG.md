# @pnpm/workspace.state

## 1001.1.19

### Patch Changes

- @pnpm/config@1003.0.1

## 1001.1.18

### Patch Changes

- Updated dependencies [56bb69b]
- Updated dependencies [8a9f3a4]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/config@1003.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/types@1000.5.0

## 1001.1.17

### Patch Changes

- @pnpm/config@1002.7.2

## 1001.1.16

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [5679712]
- Updated dependencies [01f2bcf]
  - @pnpm/types@1000.4.0
  - @pnpm/config@1002.7.1

## 1001.1.15

### Patch Changes

- Updated dependencies [e57f1df]
  - @pnpm/config@1002.7.0

## 1001.1.14

### Patch Changes

- Updated dependencies [9bcca9f]
- Updated dependencies [5b35dff]
- Updated dependencies [9bcca9f]
- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/config@1002.6.0
  - @pnpm/types@1000.3.0

## 1001.1.13

### Patch Changes

- Updated dependencies [936430a]
  - @pnpm/config@1002.5.4

## 1001.1.12

### Patch Changes

- 9904675: `@pnpm/logger` should be a peer dependency.

## 1001.1.11

### Patch Changes

- Updated dependencies [6e4459c]
  - @pnpm/config@1002.5.3

## 1001.1.10

### Patch Changes

- @pnpm/config@1002.5.2

## 1001.1.9

### Patch Changes

- Updated dependencies [c3aa4d8]
  - @pnpm/config@1002.5.1

## 1001.1.8

### Patch Changes

- Updated dependencies [a5e4965]
- Updated dependencies [d965748]
  - @pnpm/types@1000.2.1
  - @pnpm/config@1002.5.0

## 1001.1.7

### Patch Changes

- Updated dependencies [1c2eb8c]
  - @pnpm/config@1002.4.1

## 1001.1.6

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [e32b1a2]
- Updated dependencies [8fcc221]
  - @pnpm/config@1002.4.0
  - @pnpm/types@1000.2.0

## 1001.1.5

### Patch Changes

- Updated dependencies [fee898f]
  - @pnpm/config@1002.3.1

## 1001.1.4

### Patch Changes

- Updated dependencies [f6006f2]
  - @pnpm/config@1002.3.0

## 1001.1.3

### Patch Changes

- @pnpm/config@1002.2.1

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
