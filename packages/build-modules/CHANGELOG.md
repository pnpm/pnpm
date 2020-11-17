# @pnpm/build-modules

## 5.2.5

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/store-controller-types@9.2.0

## 5.2.4

### Patch Changes

- @pnpm/link-bins@5.3.20
- @pnpm/read-package-json@3.1.8
- @pnpm/lifecycle@9.6.2

## 5.2.3

### Patch Changes

- @pnpm/link-bins@5.3.19

## 5.2.2

### Patch Changes

- @pnpm/link-bins@5.3.18

## 5.2.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/core-loggers@5.0.2
  - @pnpm/lifecycle@9.6.1
  - @pnpm/link-bins@5.3.17
  - @pnpm/read-package-json@3.1.7
  - @pnpm/store-controller-types@9.1.2

## 5.2.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/lifecycle@9.6.0

## 5.1.2

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
- Updated dependencies [212671848]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/read-package-json@3.1.6
  - @pnpm/core-loggers@5.0.1
  - @pnpm/lifecycle@9.5.1
  - @pnpm/link-bins@5.3.16
  - @pnpm/store-controller-types@9.1.1

## 5.1.1

### Patch Changes

- Updated dependencies [fb863fae4]
  - @pnpm/link-bins@5.3.15

## 5.1.0

### Minor Changes

- f591fdeeb: New option added: extraEnv. extraEnv allows to pass environment variables that will be set for the child process.

### Patch Changes

- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
  - @pnpm/lifecycle@9.5.0

## 5.0.19

### Patch Changes

- Updated dependencies [51311d3ba]
  - @pnpm/link-bins@5.3.14

## 5.0.18

### Patch Changes

- 203e65ac8: The INIT_CWD env variable is always set to the lockfile directory, for scripts of dependencies.
- Updated dependencies [203e65ac8]
  - @pnpm/lifecycle@9.4.0

## 5.0.17

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/lifecycle@9.3.0

## 5.0.16

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0

## 5.0.15

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/lifecycle@9.2.5

## 5.0.14

### Patch Changes

- @pnpm/link-bins@5.3.13
- @pnpm/read-package-json@3.1.5
- @pnpm/lifecycle@9.2.4

## 5.0.13

### Patch Changes

- Updated dependencies [9f5803187]
  - @pnpm/read-package-json@3.1.4
  - @pnpm/lifecycle@9.2.3
  - @pnpm/link-bins@5.3.12

## 5.0.12

### Patch Changes

- @pnpm/link-bins@5.3.11

## 5.0.11

### Patch Changes

- @pnpm/link-bins@5.3.10

## 5.0.10

### Patch Changes

- @pnpm/link-bins@5.3.9

## 5.0.9

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/lifecycle@9.2.2
  - @pnpm/link-bins@5.3.8

## 5.0.8

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/lifecycle@9.2.1

## 5.0.7

### Patch Changes

- Updated dependencies [76aaead32]
  - @pnpm/lifecycle@9.2.0

## 5.0.6

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/lifecycle@9.1.3
  - @pnpm/link-bins@5.3.7
  - @pnpm/read-package-json@3.1.3
  - @pnpm/store-controller-types@8.0.2

## 5.0.5

### Patch Changes

- @pnpm/link-bins@5.3.6

## 5.0.4

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [e1ca9fc13]
  - @pnpm/types@6.1.0
  - @pnpm/link-bins@5.3.5
  - @pnpm/core-loggers@4.1.1
  - @pnpm/lifecycle@9.1.2
  - @pnpm/read-package-json@3.1.2
  - @pnpm/store-controller-types@8.0.1

## 5.0.3

### Patch Changes

- @pnpm/link-bins@5.3.4

## 5.0.2

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
- Updated dependencies [68d8dc68f]
  - @pnpm/lifecycle@9.1.1
  - @pnpm/core-loggers@4.1.0

## 5.0.1

### Patch Changes

- Updated dependencies [8094b2a62]
  - @pnpm/lifecycle@9.1.0

## 5.0.0

### Major Changes

- bb59db642: `peripheralLocation` in `DependenciesGraphNode` renamed to `dir`.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.
- e3990787a: Rename NodeModules to Modules in option names.

### Minor Changes

- 9b1b520d9: `packageId` removed from `DependenciesGraphNode`.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [e3990787a]
  - @pnpm/constants@4.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/link-bins@5.3.3
  - @pnpm/read-package-json@3.1.1

## 5.0.0-alpha.5

### Major Changes

- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Patch Changes

- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.4

## 5.0.0-alpha.4

### Major Changes

- e3990787: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [da091c71]
- Updated dependencies [e3990787]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0

## 4.1.15-alpha.3

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0

## 4.1.14-alpha.2

### Patch Changes

- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.2

## 4.1.14-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 4.1.14-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 4.1.14

### Patch Changes

- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0

## 4.1.13

### Patch Changes

- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/link-bins@5.3.2
