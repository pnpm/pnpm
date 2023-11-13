# @pnpm/deps.graph-builder

## 0.2.3

### Patch Changes

- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1

## 0.2.2

### Patch Changes

- fe1f0f734: Fixed a performance regression on running installation on a project with an up to date lockfile [#7297](https://github.com/pnpm/pnpm/issues/7297).
- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2

## 0.2.1

### Patch Changes

- Updated dependencies [4c2450208]
- Updated dependencies [7ea45afbe]
  - @pnpm/lockfile-utils@9.0.0
  - @pnpm/store-controller-types@17.1.1

## 0.2.0

### Minor Changes

- 43ce9e4a6: Support for multiple architectures when installing dependencies [#5965](https://github.com/pnpm/pnpm/issues/5965).

  You can now specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install. Use the `supportedArchitectures` field in `package.json` to define your preferences.

  For example, the following configuration tells pnpm to install optional dependencies for Windows x64:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32"],
        "cpu": ["x64"]
      }
    }
  }
  ```

  Whereas this configuration will have pnpm install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32", "darwin", "current"],
        "cpu": ["x64", "arm64"]
      }
    }
  }
  ```

  Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/package-is-installable@8.1.0
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-file@8.1.4
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/core-loggers@9.0.4
  - @pnpm/dependency-path@2.1.5
  - @pnpm/modules-yaml@12.1.4

## 0.1.5

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/package-is-installable@8.0.5
  - @pnpm/lockfile-file@8.1.3
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/core-loggers@9.0.3
  - @pnpm/dependency-path@2.1.4
  - @pnpm/modules-yaml@12.1.3
  - @pnpm/store-controller-types@17.0.1

## 0.1.4

### Patch Changes

- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5

## 0.1.3

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0

## 0.1.2

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0

## 0.1.1

### Patch Changes

- @pnpm/store-controller-types@16.0.1

## 0.1.0

### Minor Changes

- 494f87544: Breaking changes to the API.

### Patch Changes

- Updated dependencies [494f87544]
- Updated dependencies [e9aa6f682]
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/lockfile-utils@8.0.4

## 0.0.1

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/package-is-installable@8.0.4
  - @pnpm/lockfile-file@8.1.2
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/core-loggers@9.0.2
  - @pnpm/dependency-path@2.1.3
  - @pnpm/modules-yaml@12.1.2
  - @pnpm/store-controller-types@15.0.2
