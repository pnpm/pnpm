# @pnpm/default-reporter

## 7.9.10

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0

## 7.9.9

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0

## 7.9.8

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/config@11.7.2

## 7.9.7

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/config@11.7.1
  - @pnpm/core-loggers@5.0.2

## 7.9.6

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0

## 7.9.5

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/config@11.6.1
  - @pnpm/core-loggers@5.0.1

## 7.9.4

### Patch Changes

- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0

## 7.9.3

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0

## 7.9.2

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0

## 7.9.1

### Patch Changes

- 3b8e3b6b1: Always print the final progress stats.
- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0

## 7.9.0

### Minor Changes

- 663afd68e: Scope is not reported when the scope is only one project.
- 86cd72de3: Show the progress of adding packages to the virtual store.

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0

## 7.8.0

### Minor Changes

- 09b42d3ab: Use RxJS instead of "most".

## 7.7.0

### Minor Changes

- af8361946: Sometimes, when installing new dependencies that rely on many peer dependencies, or when running installation on a huge monorepo, there will be hundreds or thousands of warnings. Printing many messages to the terminal is expensive and reduces speed, so pnpm will only print a few warnings and report the total number of the unprinted warnings.

## 7.6.4

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7

## 7.6.3

### Patch Changes

- 13c332e69: Fixes a regression published in pnpm v5.5.3 as a result of nullish coalescing refactoring.

## 7.6.2

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6

## 7.6.1

### Patch Changes

- Updated dependencies [972864e0d]
  - @pnpm/config@11.2.5

## 7.6.0

### Minor Changes

- 6d480dd7a: Print the authorization settings (with hidden private info), when an authorization error happens during fetch.

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/config@11.2.4

## 7.5.4

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3

## 7.5.3

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2

## 7.5.2

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/config@11.2.1

## 7.5.1

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/config@11.2.0

## 7.5.0

### Minor Changes

- 9a908bc07: Print info after install about hardlinked/copied packages in `node_modules/.pnpm`

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 7.4.7

### Patch Changes

- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/config@11.1.0

## 7.4.6

### Patch Changes

- @pnpm/config@11.0.1

## 7.4.5

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0

## 7.4.4

### Patch Changes

- @pnpm/config@10.0.1

## 7.4.3

### Patch Changes

- 220896511: Remove common-tags from dependencies.
- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/config@10.0.0
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2

## 7.4.2

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/config@9.2.0
  - @pnpm/core-loggers@4.1.1

## 7.4.1

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.

## 7.4.0

### Minor Changes

- 2ebb7af33: New reporter added for request retries.

### Patch Changes

- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0

## 7.3.0

### Minor Changes

- eb82084e1: Color the different output prefixes differently.
- ffddf34a8: Add new reporting option: `streamLifecycleOutput`. When `true`, the output from child processes is printed immediately and is never collapsed.

### Patch Changes

- Updated dependencies [ffddf34a8]
  - @pnpm/config@9.1.0

## 7.2.5

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [da091c711]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0
  - @pnpm/types@6.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/error@1.2.1

## 7.2.5-alpha.2

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2

## 7.2.5-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0

## 7.2.5-alpha.0

### Patch Changes

- @pnpm/config@8.3.1-alpha.0

## 7.2.4

### Patch Changes

- 907c63a48: Global warnings are reported.
