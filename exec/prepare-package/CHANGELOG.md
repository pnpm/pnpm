# @pnpm/prepare-package

## 5.0.12

### Patch Changes

- 17d2ddb05: Don't run the `prepublishOnly` scripts of git-hosted dependencies [#7026](https://github.com/pnpm/pnpm/issues/7026).

## 5.0.11

### Patch Changes

- Updated dependencies [9caa33d53]
  - @pnpm/lifecycle@16.0.0

## 5.0.10

### Patch Changes

- @pnpm/lifecycle@15.0.9

## 5.0.9

### Patch Changes

- @pnpm/lifecycle@15.0.8

## 5.0.8

### Patch Changes

- Updated dependencies [e9aa6f682]
  - @pnpm/lifecycle@15.0.7

## 5.0.7

### Patch Changes

- Updated dependencies [692197df3]
  - @pnpm/lifecycle@15.0.6

## 5.0.6

### Patch Changes

- 8452bb2d5: The "postpublish" script of a git-hosted dependency is not executed, while building the dependency [#6822](https://github.com/pnpm/pnpm/issues/6846).
  - @pnpm/lifecycle@15.0.5

## 5.0.5

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/lifecycle@15.0.5
  - @pnpm/read-package-json@8.0.3

## 5.0.4

### Patch Changes

- @pnpm/lifecycle@15.0.4

## 5.0.3

### Patch Changes

- Updated dependencies [dddb8ad71]
  - @pnpm/lifecycle@15.0.3

## 5.0.2

### Patch Changes

- @pnpm/lifecycle@15.0.2
- @pnpm/read-package-json@8.0.2

## 5.0.1

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [6ce3424a9]
  - @pnpm/types@9.1.0
  - @pnpm/lifecycle@15.0.1
  - @pnpm/read-package-json@8.0.1

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/read-package-json@8.0.0
  - @pnpm/lifecycle@15.0.0

## 4.1.2

### Patch Changes

- @pnpm/lifecycle@14.1.7

## 4.1.1

### Patch Changes

- @pnpm/lifecycle@14.1.6

## 4.1.0

### Minor Changes

- c7b05cd9a: When ignoreScripts=true is passed to the fetcher, do not build git-hosted dependencies.

### Patch Changes

- @pnpm/lifecycle@14.1.5
- @pnpm/read-package-json@7.0.5

## 4.0.1

### Patch Changes

- ec97a3105: Report to the console when a git-hosted dependency is built [#5847](https://github.com/pnpm/pnpm/pull/5847).
- 40a481840: Only run prepublish scripts of git-hosted dependencies, if the dependency doesn't have a main file. In this case we can assume that the dependencies has to be built.

## 4.0.0

### Major Changes

- 339c0a704: A new required option added to the prepare package function: rawConfig. It is needed in order to create a proper environment for the package manager executed during the preparation of a git-hosted dependency.

### Patch Changes

- 339c0a704: Run the prepublish scripts of packages installed from Git [#5826](https://github.com/pnpm/pnpm/issues/5826).

## 3.0.4

### Patch Changes

- @pnpm/read-package-json@7.0.4

## 3.0.3

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/read-package-json@7.0.3

## 3.0.2

### Patch Changes

- @pnpm/read-package-json@7.0.2

## 3.0.1

### Patch Changes

- @pnpm/read-package-json@7.0.1

## 3.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/read-package-json@7.0.0

## 2.0.11

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/read-package-json@6.0.11

## 2.0.10

### Patch Changes

- @pnpm/read-package-json@6.0.10

## 2.0.9

### Patch Changes

- @pnpm/read-package-json@6.0.9

## 2.0.8

### Patch Changes

- Updated dependencies [07bc24ad1]
  - @pnpm/read-package-json@6.0.8

## 2.0.7

### Patch Changes

- @pnpm/read-package-json@6.0.7

## 2.0.6

### Patch Changes

- @pnpm/read-package-json@6.0.6

## 2.0.5

### Patch Changes

- @pnpm/read-package-json@6.0.5

## 2.0.4

### Patch Changes

- @pnpm/read-package-json@6.0.4

## 2.0.3

### Patch Changes

- @pnpm/read-package-json@6.0.3

## 2.0.2

### Patch Changes

- @pnpm/read-package-json@6.0.2

## 2.0.1

### Patch Changes

- @pnpm/error@3.0.1
- @pnpm/read-package-json@6.0.1

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/error@3.0.0
  - @pnpm/read-package-json@6.0.0

## 1.0.13

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/read-package-json@5.0.12

## 1.0.12

### Patch Changes

- @pnpm/read-package-json@5.0.11

## 1.0.11

### Patch Changes

- @pnpm/read-package-json@5.0.10

## 1.0.10

### Patch Changes

- @pnpm/read-package-json@5.0.9

## 1.0.9

### Patch Changes

- eec4b195d: Always return an error message when the preparation of a package fails.
  - @pnpm/read-package-json@5.0.8

## 1.0.8

### Patch Changes

- @pnpm/read-package-json@5.0.7

## 1.0.7

### Patch Changes

- fb1a95a6c: If prepare fails, throw a pnpm error with a known error code.

## 1.0.6

### Patch Changes

- @pnpm/read-package-json@5.0.6

## 1.0.5

### Patch Changes

- @pnpm/read-package-json@5.0.5

## 1.0.4

### Patch Changes

- 4a4d42d8f: Packages that have no `package.json` files should be skipped.

## 1.0.3

### Patch Changes

- @pnpm/read-package-json@5.0.4

## 1.0.2

### Patch Changes

- @pnpm/read-package-json@5.0.3

## 1.0.1

### Patch Changes

- @pnpm/read-package-json@5.0.2

## 1.0.0

### Major Changes

- e6a2654a2: Project created.
