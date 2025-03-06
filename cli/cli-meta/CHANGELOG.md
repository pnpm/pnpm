# @pnpm/cli-meta

## 1000.0.4

### Patch Changes

- Updated dependencies [a5e4965]
  - @pnpm/types@1000.2.1

## 1000.0.3

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0

## 1000.0.2

### Patch Changes

- Updated dependencies [b562deb]
  - @pnpm/types@1000.1.1

## 1000.0.1

### Patch Changes

- Updated dependencies [9591a18]
  - @pnpm/types@1000.1.0

## 6.2.2

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0

## 6.2.1

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0

## 6.2.0

### Minor Changes

- eb8bf2a: Added a new command for upgrading pnpm itself when it isn't managed by Corepack: `pnpm self-update`. This command will work, when pnpm was installed via the standalone script from the [pnpm installation page](https://pnpm.io/installation#using-a-standalone-script) [#8424](https://github.com/pnpm/pnpm/pull/8424).

  When executed in a project that has a `packageManager` field in its `package.json` file, pnpm will update its version in the `packageManager` field.

## 6.1.0

### Minor Changes

- 64e2e4f: Added isExecutedByCorepack.
- e7f6330: Add detectIfCurrentPkgIsExecutable.

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/types@12.0.0

## 6.0.4

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0

## 6.0.3

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0

## 6.0.2

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1

## 6.0.1

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [43cdd87]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0

## 5.0.6

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2

## 5.0.5

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1

## 5.0.4

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0

## 5.0.3

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0

## 5.0.2

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0

## 5.0.1

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/types@9.0.0

## 4.0.3

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0

## 4.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0

## 4.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0

## 4.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

## 3.0.8

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0

## 3.0.7

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0

## 3.0.6

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0

## 3.0.5

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0

## 3.0.4

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0

## 3.0.3

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0

## 3.0.2

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0

## 3.0.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0

## 2.0.2

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0

## 2.0.1

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 1.0.2

### Patch Changes

- 43de80034: Don't fail when the code is executed through piping to Node's stdin.

## 1.0.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 1.0.0
