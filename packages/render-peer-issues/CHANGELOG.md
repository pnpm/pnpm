# @pnpm/render-peer-issues

## 5.0.4

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0

## 5.0.3

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1

## 5.0.2

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0

## 5.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/parse-overrides@5.0.1

## 5.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- aa33269: Peer dependency rules should only affect reporting, not data in the lockfile.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/parse-overrides@5.0.0
  - @pnpm/matcher@6.0.0

## 4.0.6

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2

## 4.0.5

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1

## 4.0.4

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0

## 4.0.3

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0

## 4.0.2

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0

## 4.0.1

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0

## 4.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/types@9.0.0

## 3.0.3

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0

## 3.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0

## 3.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0

## 3.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 2.1.2

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0

## 2.1.1

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0

## 2.1.0

### Minor Changes

- c990a409f: Print the versions of packages in peer dependency warnings and errors.

## 2.0.6

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0

## 2.0.5

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0

## 2.0.4

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0

## 2.0.3

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0

## 2.0.2

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0

## 2.0.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0

## 1.1.2

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0

## 1.1.1

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0

## 1.1.0

### Minor Changes

- b5734a4a7: When reporting unmet peer dependency issues, if the peer dependency is resolved not from a dependency installed by the user, then print the name of the parent package that has the bad peer dependency installed as a dependency.

  ![](https://i.imgur.com/0kjij22.png)

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0

## 1.0.2

### Patch Changes

- 6058f76cd: When printing peer dependency issues, print the "\*" range in double quotes. This will make it easier to copy the package resolutions and put them to the end of a `pnpm add` command for execution.

## 1.0.1

### Patch Changes

- a087f339e: A new line should be between the summary about conflicting peers and non-conflicting ones.
- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1

## 1.0.0

### Major Changes

- ba9b2eba1: Initial release.

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
