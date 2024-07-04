# @pnpm/filter-lockfile

## 9.0.8

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/package-is-installable@9.0.4
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/lockfile-walker@9.0.3
  - @pnpm/dependency-path@5.1.2

## 9.0.7

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/package-is-installable@9.0.3
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/lockfile-walker@9.0.2
  - @pnpm/dependency-path@5.1.1

## 9.0.6

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/lockfile-utils@11.0.1
  - @pnpm/lockfile-walker@9.0.1

## 9.0.5

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/lockfile-walker@9.0.0
  - @pnpm/dependency-path@5.0.0
  - @pnpm/package-is-installable@9.0.2

## 9.0.4

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/package-is-installable@9.0.1

## 9.0.3

### Patch Changes

- Updated dependencies [7a0536e]
  - @pnpm/lockfile-utils@10.1.1

## 9.0.2

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/lockfile-utils@10.1.0
  - @pnpm/lockfile-walker@8.0.1

## 9.0.1

### Patch Changes

- b7d2ed4: The `engines.pnpm` field in the `package.json` files of dependencies should be ignored [#7965](https://github.com/pnpm/pnpm/issues/7965).

## 9.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/package-is-installable@9.0.0
  - @pnpm/lockfile-walker@8.0.0
  - @pnpm/lockfile-types@6.0.0

## 8.1.6

### Patch Changes

- @pnpm/lockfile-utils@9.0.5

## 8.1.5

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/lockfile-utils@9.0.4
  - @pnpm/lockfile-walker@7.0.8
  - @pnpm/package-is-installable@8.1.2
  - @pnpm/dependency-path@2.1.7

## 8.1.4

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/lockfile-utils@9.0.3
  - @pnpm/lockfile-walker@7.0.7
  - @pnpm/package-is-installable@8.1.1
  - @pnpm/dependency-path@2.1.6

## 8.1.3

### Patch Changes

- Updated dependencies [d5a176af7]
  - @pnpm/lockfile-utils@9.0.2

## 8.1.2

### Patch Changes

- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1

## 8.1.1

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/lockfile-utils@9.0.0

## 8.1.0

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
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/lockfile-walker@7.0.6
  - @pnpm/dependency-path@2.1.5

## 8.0.10

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/package-is-installable@8.0.5
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/lockfile-walker@7.0.5
  - @pnpm/dependency-path@2.1.4

## 8.0.9

### Patch Changes

- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5

## 8.0.8

### Patch Changes

- Updated dependencies [e9aa6f682]
  - @pnpm/lockfile-utils@8.0.4

## 8.0.7

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/package-is-installable@8.0.4
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/lockfile-walker@7.0.4
  - @pnpm/dependency-path@2.1.3

## 8.0.6

### Patch Changes

- Updated dependencies [d9da627cd]
- Updated dependencies [302ebffc5]
  - @pnpm/lockfile-utils@8.0.2
  - @pnpm/constants@7.1.1
  - @pnpm/error@5.0.2
  - @pnpm/package-is-installable@8.0.3

## 8.0.5

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/lockfile-utils@8.0.1
  - @pnpm/lockfile-walker@7.0.3
  - @pnpm/package-is-installable@8.0.2
  - @pnpm/dependency-path@2.1.2
  - @pnpm/error@5.0.1

## 8.0.4

### Patch Changes

- Updated dependencies [d58cdb962]
  - @pnpm/lockfile-utils@8.0.0

## 8.0.3

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/package-is-installable@8.0.1
  - @pnpm/dependency-path@2.1.1
  - @pnpm/lockfile-utils@7.0.1
  - @pnpm/lockfile-walker@7.0.2

## 8.0.2

### Patch Changes

- Updated dependencies [72ba638e3]
  - @pnpm/lockfile-utils@7.0.0

## 8.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/lockfile-utils@6.0.1
  - @pnpm/lockfile-walker@7.0.1

## 8.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/package-is-installable@8.0.0
  - @pnpm/lockfile-walker@7.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 7.0.10

### Patch Changes

- @pnpm/lockfile-utils@5.0.7

## 7.0.9

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/lockfile-utils@5.0.6
  - @pnpm/lockfile-walker@6.0.8

## 7.0.8

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2
  - @pnpm/lockfile-utils@5.0.5
  - @pnpm/lockfile-walker@6.0.7

## 7.0.7

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/lockfile-utils@5.0.4
  - @pnpm/lockfile-walker@6.0.6

## 7.0.6

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/error@4.0.1
  - @pnpm/lockfile-utils@5.0.3
  - @pnpm/lockfile-walker@6.0.5
  - @pnpm/package-is-installable@7.0.4

## 7.0.5

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/package-is-installable@7.0.3
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/lockfile-utils@5.0.2
  - @pnpm/lockfile-walker@6.0.4
  - @pnpm/dependency-path@1.0.1

## 7.0.4

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/lockfile-utils@5.0.1
  - @pnpm/lockfile-walker@6.0.3

## 7.0.3

### Patch Changes

- Updated dependencies [ecc8794bb]
- Updated dependencies [ecc8794bb]
  - @pnpm/lockfile-utils@5.0.0

## 7.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - dependency-path@9.2.8
  - @pnpm/lockfile-types@4.3.5
  - @pnpm/lockfile-utils@4.2.8
  - @pnpm/lockfile-walker@6.0.2
  - @pnpm/package-is-installable@7.0.2

## 7.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - dependency-path@9.2.7
  - @pnpm/lockfile-types@4.3.4
  - @pnpm/lockfile-utils@4.2.7
  - @pnpm/lockfile-walker@6.0.1
  - @pnpm/package-is-installable@7.0.1

## 7.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.
- a236ecf57: Breaking change to the API. Also include missing deeply linked workspace packages at headless installation.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/lockfile-walker@6.0.0
  - @pnpm/package-is-installable@7.0.0

## 6.0.22

### Patch Changes

- @pnpm/package-is-installable@6.0.12

## 6.0.21

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/package-is-installable@6.0.11

## 6.0.20

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - dependency-path@9.2.6
  - @pnpm/lockfile-types@4.3.3
  - @pnpm/lockfile-utils@4.2.6
  - @pnpm/lockfile-walker@5.0.15
  - @pnpm/package-is-installable@6.0.10

## 6.0.19

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - dependency-path@9.2.5
  - @pnpm/lockfile-types@4.3.2
  - @pnpm/lockfile-utils@4.2.5
  - @pnpm/lockfile-walker@5.0.14
  - @pnpm/package-is-installable@6.0.9

## 6.0.18

### Patch Changes

- 1beb1b4bd: Auto install peer dependencies when auto-install-peers is set to true and the lockfile is up to date [#5213](https://github.com/pnpm/pnpm/issues/5213).

## 6.0.17

### Patch Changes

- @pnpm/lockfile-utils@4.2.4

## 6.0.16

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
  - @pnpm/lockfile-utils@4.2.3
  - @pnpm/lockfile-walker@5.0.13

## 6.0.15

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - dependency-path@9.2.4
  - @pnpm/lockfile-types@4.3.1
  - @pnpm/lockfile-utils@4.2.2
  - @pnpm/lockfile-walker@5.0.12
  - @pnpm/package-is-installable@6.0.8

## 6.0.14

### Patch Changes

- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1

## 6.0.13

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0
  - @pnpm/lockfile-utils@4.2.0
  - @pnpm/lockfile-walker@5.0.11

## 6.0.12

### Patch Changes

- Updated dependencies [e3f4d131c]
  - @pnpm/lockfile-utils@4.1.0

## 6.0.11

### Patch Changes

- dependency-path@9.2.3
- @pnpm/lockfile-utils@4.0.10
- @pnpm/lockfile-walker@5.0.10

## 6.0.10

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/lockfile-utils@4.0.9
  - @pnpm/lockfile-walker@5.0.9

## 6.0.9

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2
  - @pnpm/lockfile-utils@4.0.8
  - @pnpm/lockfile-walker@5.0.8

## 6.0.8

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - @pnpm/lockfile-utils@4.0.7
  - @pnpm/lockfile-walker@5.0.7
  - dependency-path@9.2.1
  - @pnpm/package-is-installable@6.0.7

## 6.0.7

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - dependency-path@9.2.0
  - @pnpm/lockfile-utils@4.0.6
  - @pnpm/lockfile-walker@5.0.6
  - @pnpm/package-is-installable@6.0.6

## 6.0.6

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - dependency-path@9.1.4
  - @pnpm/lockfile-types@4.0.3
  - @pnpm/lockfile-utils@4.0.5
  - @pnpm/lockfile-walker@5.0.5
  - @pnpm/package-is-installable@6.0.5

## 6.0.5

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - dependency-path@9.1.3
  - @pnpm/lockfile-types@4.0.2
  - @pnpm/lockfile-utils@4.0.4
  - @pnpm/lockfile-walker@5.0.4
  - @pnpm/package-is-installable@6.0.4

## 6.0.4

### Patch Changes

- Updated dependencies [c57695550]
  - dependency-path@9.1.2
  - @pnpm/lockfile-utils@4.0.3
  - @pnpm/lockfile-walker@5.0.3

## 6.0.3

### Patch Changes

- 52b0576af: feat: support libc filed
- Updated dependencies [52b0576af]
  - @pnpm/package-is-installable@6.0.3

## 6.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - dependency-path@9.1.1
  - @pnpm/lockfile-types@4.0.1
  - @pnpm/lockfile-utils@4.0.2
  - @pnpm/lockfile-walker@5.0.2
  - @pnpm/package-is-installable@6.0.2

## 6.0.1

### Patch Changes

- Updated dependencies [0a70aedb1]
- Updated dependencies [688b0eaff]
- Updated dependencies [1267e4eff]
  - dependency-path@9.1.0
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/constants@6.1.0
  - @pnpm/lockfile-walker@5.0.1
  - @pnpm/error@3.0.1
  - @pnpm/package-is-installable@6.0.1

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/error@3.0.0
  - @pnpm/lockfile-types@4.0.0
  - @pnpm/lockfile-utils@4.0.0
  - @pnpm/lockfile-walker@5.0.0
  - @pnpm/package-is-installable@6.0.0

## 5.0.19

### Patch Changes

- 70ba51da9: Update `@pnpm/error`.
- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/package-is-installable@5.0.13

## 5.0.18

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - @pnpm/lockfile-utils@3.2.1
  - @pnpm/lockfile-walker@4.0.15
  - dependency-path@8.0.11
  - @pnpm/package-is-installable@5.0.12

## 5.0.17

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/lockfile-utils@3.2.0

## 5.0.16

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - dependency-path@8.0.10
  - @pnpm/lockfile-types@3.1.5
  - @pnpm/lockfile-utils@3.1.6
  - @pnpm/lockfile-walker@4.0.14
  - @pnpm/package-is-installable@5.0.11

## 5.0.15

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - dependency-path@8.0.9
  - @pnpm/lockfile-types@3.1.4
  - @pnpm/lockfile-utils@3.1.5
  - @pnpm/lockfile-walker@4.0.13
  - @pnpm/package-is-installable@5.0.10

## 5.0.14

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - dependency-path@8.0.8
  - @pnpm/lockfile-types@3.1.3
  - @pnpm/lockfile-utils@3.1.4
  - @pnpm/lockfile-walker@4.0.12
  - @pnpm/package-is-installable@5.0.9

## 5.0.13

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/package-is-installable@5.0.8
  - dependency-path@8.0.7
  - @pnpm/lockfile-types@3.1.2
  - @pnpm/lockfile-utils@3.1.3
  - @pnpm/lockfile-walker@4.0.11

## 5.0.12

### Patch Changes

- Updated dependencies [3cf543fc1]
  - @pnpm/lockfile-utils@3.1.2

## 5.0.11

### Patch Changes

- Updated dependencies [783cc1051]
  - @pnpm/package-is-installable@5.0.7

## 5.0.10

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - dependency-path@8.0.6
  - @pnpm/lockfile-types@3.1.1
  - @pnpm/lockfile-utils@3.1.1
  - @pnpm/lockfile-walker@4.0.10
  - @pnpm/package-is-installable@5.0.6

## 5.0.9

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-types@3.1.0
  - @pnpm/lockfile-utils@3.1.0
  - dependency-path@8.0.5
  - @pnpm/lockfile-walker@4.0.9
  - @pnpm/package-is-installable@5.0.5

## 5.0.8

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - dependency-path@8.0.4
  - @pnpm/lockfile-utils@3.0.8
  - @pnpm/lockfile-walker@4.0.8
  - @pnpm/package-is-installable@5.0.4

## 5.0.7

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - dependency-path@8.0.3
  - @pnpm/lockfile-utils@3.0.7
  - @pnpm/lockfile-walker@4.0.7
  - @pnpm/package-is-installable@5.0.3

## 5.0.6

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2
  - @pnpm/lockfile-utils@3.0.6
  - @pnpm/lockfile-walker@4.0.6

## 5.0.5

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - dependency-path@8.0.1
  - @pnpm/lockfile-utils@3.0.5
  - @pnpm/lockfile-walker@4.0.5
  - @pnpm/package-is-installable@5.0.2

## 5.0.4

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/lockfile-utils@3.0.4
  - @pnpm/lockfile-walker@4.0.4

## 5.0.3

### Patch Changes

- Updated dependencies [20e2f235d]
  - dependency-path@8.0.0
  - @pnpm/lockfile-utils@3.0.3
  - @pnpm/lockfile-walker@4.0.3

## 5.0.2

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - dependency-path@7.0.1
  - @pnpm/lockfile-utils@3.0.2
  - @pnpm/lockfile-walker@4.0.2
  - @pnpm/package-is-installable@5.0.1

## 5.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0
  - @pnpm/lockfile-utils@3.0.1
  - @pnpm/lockfile-walker@4.0.1

## 5.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - dependency-path@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/lockfile-utils@3.0.0
  - @pnpm/lockfile-walker@4.0.0
  - @pnpm/package-is-installable@5.0.0
  - @pnpm/types@7.0.0

## 4.0.17

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - @pnpm/lockfile-utils@2.0.22
  - @pnpm/lockfile-walker@3.0.9
  - dependency-path@5.1.1
  - @pnpm/package-is-installable@4.0.19

## 4.0.16

### Patch Changes

- af897c324: Include all the properties of the filtered lockfile.

## 4.0.15

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0
  - @pnpm/lockfile-utils@2.0.21
  - @pnpm/lockfile-walker@3.0.8

## 4.0.14

### Patch Changes

- @pnpm/lockfile-utils@2.0.20

## 4.0.13

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/package-is-installable@4.0.18

## 4.0.12

### Patch Changes

- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6
  - @pnpm/lockfile-utils@2.0.19
  - @pnpm/lockfile-walker@3.0.7

## 4.0.11

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - @pnpm/lockfile-utils@2.0.18
  - @pnpm/lockfile-walker@3.0.6
  - dependency-path@5.0.5
  - @pnpm/package-is-installable@4.0.17

## 4.0.10

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/lockfile-utils@2.0.17
  - @pnpm/lockfile-walker@3.0.5
  - dependency-path@5.0.4
  - @pnpm/package-is-installable@4.0.16

## 4.0.9

### Patch Changes

- @pnpm/package-is-installable@4.0.15

## 4.0.8

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/package-is-installable@4.0.14

## 4.0.7

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/package-is-installable@4.0.13

## 4.0.6

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [1140ef721]
- Updated dependencies [a2ef8084f]
  - @pnpm/lockfile-utils@2.0.16
  - dependency-path@5.0.3
  - @pnpm/lockfile-walker@3.0.4

## 4.0.5

### Patch Changes

- @pnpm/package-is-installable@4.0.12

## 4.0.4

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - dependency-path@5.0.2
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/lockfile-walker@3.0.3
  - @pnpm/package-is-installable@4.0.11

## 4.0.3

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - dependency-path@5.0.1
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/lockfile-walker@3.0.2
  - @pnpm/package-is-installable@4.0.10

## 4.0.2

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/lockfile-utils@2.0.13
  - @pnpm/lockfile-walker@3.0.1

## 4.0.1

### Patch Changes

- @pnpm/package-is-installable@4.0.9

## 4.0.0

### Major Changes

- c25cccdad: `filterLockfileByImportersAndEngine()` does not remove the skipped packages from the filtered lockfile.
- 2485eaf60: `opts.registries` is not needed. `opts.skipped` should be relative dependency paths.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [142f8caf7]
- Updated dependencies [da091c711]
- Updated dependencies [6a8a97eee]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/lockfile-walker@3.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - dependency-path@4.0.7
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/package-is-installable@4.0.8

## 4.0.0-alpha.2

### Major Changes

- c25cccdad: `filterLockfileByImportersAndEngine()` does not remove the skipped packages from the filtered lockfile.
- 2485eaf60: `opts.registries` is not needed. `opts.skipped` should be relative dependency paths.

### Patch Changes

- Updated dependencies [ca9f50844]
- Updated dependencies [6a8a97eee]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.1
  - @pnpm/lockfile-walker@2.0.3-alpha.1

## 3.2.3-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/lockfile-walker@2.0.3-alpha.0
  - @pnpm/package-is-installable@4.0.8-alpha.0

## 3.2.3-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0

## 3.2.2

### Patch Changes

- 907c63a48: Dependencies updated.
- Updated dependencies [907c63a48]
  - @pnpm/lockfile-utils@2.0.11
