# @pnpm/real-hoist

## 3.0.7

### Patch Changes

- @pnpm/lockfile-utils@11.0.3
- @pnpm/dependency-path@5.1.2

## 3.0.6

### Patch Changes

- @pnpm/lockfile-utils@11.0.2
- @pnpm/dependency-path@5.1.1

## 3.0.5

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-utils@11.0.1

## 3.0.4

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0

## 3.0.3

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 3.0.2

### Patch Changes

- Updated dependencies [7a0536e]
  - @pnpm/lockfile-utils@10.1.1

## 3.0.1

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/lockfile-utils@10.1.0

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [d381a60]
- Updated dependencies [98a1266]
  - @pnpm/error@6.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0

## 2.0.19

### Patch Changes

- @pnpm/lockfile-utils@9.0.5

## 2.0.18

### Patch Changes

- @pnpm/lockfile-utils@9.0.4
- @pnpm/dependency-path@2.1.7

## 2.0.17

### Patch Changes

- @pnpm/lockfile-utils@9.0.3
- @pnpm/dependency-path@2.1.6

## 2.0.16

### Patch Changes

- Updated dependencies [d5a176af7]
  - @pnpm/lockfile-utils@9.0.2

## 2.0.15

### Patch Changes

- b4194fe52: Fixed out-of-memory exception that was happening on dependencies with many peer dependencies, when `node-linker` was set to `hoisted` [#6227](https://github.com/pnpm/pnpm/issues/6227).
- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1

## 2.0.14

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/lockfile-utils@9.0.0

## 2.0.13

### Patch Changes

- @pnpm/lockfile-utils@8.0.7
- @pnpm/dependency-path@2.1.5

## 2.0.12

### Patch Changes

- @pnpm/lockfile-utils@8.0.6
- @pnpm/dependency-path@2.1.4

## 2.0.11

### Patch Changes

- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5

## 2.0.10

### Patch Changes

- Updated dependencies [e9aa6f682]
  - @pnpm/lockfile-utils@8.0.4

## 2.0.9

### Patch Changes

- @pnpm/lockfile-utils@8.0.3
- @pnpm/dependency-path@2.1.3

## 2.0.8

### Patch Changes

- 59aba9e72: Peer dependencies of subdependencies should be installed, when `node-linker` is set to `hoisted` [#6680](https://github.com/pnpm/pnpm/pull/6680).

## 2.0.7

### Patch Changes

- Updated dependencies [d9da627cd]
  - @pnpm/lockfile-utils@8.0.2
  - @pnpm/error@5.0.2

## 2.0.6

### Patch Changes

- d55b41a8b: Dependencies have been updated.

## 2.0.5

### Patch Changes

- @pnpm/lockfile-utils@8.0.1
- @pnpm/dependency-path@2.1.2
- @pnpm/error@5.0.1

## 2.0.4

### Patch Changes

- Updated dependencies [d58cdb962]
  - @pnpm/lockfile-utils@8.0.0

## 2.0.3

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/dependency-path@2.1.1
  - @pnpm/lockfile-utils@7.0.1

## 2.0.2

### Patch Changes

- e440d784f: Update yarn dependencies.
- Updated dependencies [72ba638e3]
  - @pnpm/lockfile-utils@7.0.0

## 2.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/lockfile-utils@6.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/error@5.0.0

## 1.1.6

### Patch Changes

- @pnpm/lockfile-utils@5.0.7

## 1.1.5

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/lockfile-utils@5.0.6

## 1.1.4

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2
  - @pnpm/lockfile-utils@5.0.5

## 1.1.3

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/lockfile-utils@5.0.4

## 1.1.2

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/dependency-path@1.1.0
  - @pnpm/error@4.0.1
  - @pnpm/lockfile-utils@5.0.3

## 1.1.1

### Patch Changes

- @pnpm/lockfile-utils@5.0.2
- @pnpm/dependency-path@1.0.1

## 1.1.0

### Minor Changes

- 450e0b1d1: A new option added for avoiding hoisting some dependencies to the root of `node_modules`: `externalDependencies`. This option is a set of dependency names that were added to `node_modules` by another tool. pnpm doesn't have information about these dependencies but they shouldn't be overwritten by hoisted dependencies.

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/lockfile-utils@5.0.1

## 1.0.4

### Patch Changes

- Updated dependencies [ecc8794bb]
- Updated dependencies [ecc8794bb]
  - @pnpm/lockfile-utils@5.0.0

## 1.0.3

### Patch Changes

- dependency-path@9.2.8
- @pnpm/lockfile-utils@4.2.8

## 1.0.2

### Patch Changes

- 0da2f0412: Update dependencies.

## 1.0.1

### Patch Changes

- dependency-path@9.2.7
- @pnpm/lockfile-utils@4.2.7

## 1.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- e35988d1f: Update Yarn dependencies.
- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0

## 0.2.20

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0

## 0.2.19

### Patch Changes

- dependency-path@9.2.6
- @pnpm/lockfile-utils@4.2.6

## 0.2.18

### Patch Changes

- dependency-path@9.2.5
- @pnpm/lockfile-utils@4.2.5

## 0.2.17

### Patch Changes

- 9faf0221d: Update Yarn dependencies.

## 0.2.16

### Patch Changes

- @pnpm/lockfile-utils@4.2.4

## 0.2.15

### Patch Changes

- Updated dependencies [8103f92bd]
  - @pnpm/lockfile-utils@4.2.3

## 0.2.14

### Patch Changes

- dependency-path@9.2.4
- @pnpm/lockfile-utils@4.2.2

## 0.2.13

### Patch Changes

- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1

## 0.2.12

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-utils@4.2.0

## 0.2.11

### Patch Changes

- Updated dependencies [e3f4d131c]
  - @pnpm/lockfile-utils@4.1.0

## 0.2.10

### Patch Changes

- dependency-path@9.2.3
- @pnpm/lockfile-utils@4.0.10

## 0.2.9

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/lockfile-utils@4.0.9

## 0.2.8

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2
  - @pnpm/lockfile-utils@4.0.8

## 0.2.7

### Patch Changes

- @pnpm/lockfile-utils@4.0.7
- dependency-path@9.2.1

## 0.2.6

### Patch Changes

- Updated dependencies [c635f9fc1]
  - dependency-path@9.2.0
  - @pnpm/lockfile-utils@4.0.6

## 0.2.5

### Patch Changes

- Updated dependencies [725636a90]
  - dependency-path@9.1.4
  - @pnpm/lockfile-utils@4.0.5

## 0.2.4

### Patch Changes

- dependency-path@9.1.3
- @pnpm/lockfile-utils@4.0.4

## 0.2.3

### Patch Changes

- Updated dependencies [c57695550]
  - dependency-path@9.1.2
  - @pnpm/lockfile-utils@4.0.3

## 0.2.2

### Patch Changes

- dependency-path@9.1.1
- @pnpm/lockfile-utils@4.0.2

## 0.2.1

### Patch Changes

- Updated dependencies [0a70aedb1]
- Updated dependencies [688b0eaff]
  - dependency-path@9.1.0
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/error@3.0.1

## 0.2.0

### Minor Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- 9b9b13c3a: Update Yarn dependencies.
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - dependency-path@9.0.0
  - @pnpm/error@3.0.0
  - @pnpm/lockfile-utils@4.0.0

## 0.1.8

### Patch Changes

- 70ba51da9: Throw a meaningful error message on `pnpm install` when the lockfile is broken and `node-linker` is set to `hoisted`.
- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0

## 0.1.7

### Patch Changes

- @pnpm/lockfile-utils@3.2.1
- dependency-path@8.0.11

## 0.1.6

### Patch Changes

- 329e186e9: Allow to set hoistingLimits for the hoisted node linker.

## 0.1.5

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/lockfile-utils@3.2.0

## 0.1.4

### Patch Changes

- 6b877aad5: Update `@yarnpkg/nm` to `v3.0.1-rc.10`.

## 0.1.3

### Patch Changes

- dependency-path@8.0.10
- @pnpm/lockfile-utils@3.1.6

## 0.1.2

### Patch Changes

- cbd2f3e2a: Downgrade and pin Yarn lib versions.

## 0.1.1

### Patch Changes

- 1018ec1fd: When the same package is installed through different aliases, hoist each of the aliases.

## 0.1.0

### Minor Changes

- 732d4962f: Initial release.
