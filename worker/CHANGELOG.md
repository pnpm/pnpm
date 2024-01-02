# @pnpm/worker

## 0.3.13

### Patch Changes

- @pnpm/create-cafs-store@6.0.12

## 0.3.12

### Patch Changes

- Updated dependencies [33313d2fd]
  - @pnpm/store.cafs@2.0.11
  - @pnpm/create-cafs-store@6.0.11
  - @pnpm/symlink-dependency@7.1.4
  - @pnpm/cafs-types@4.0.0
  - @pnpm/fs.hard-link-dir@3.0.0

## 0.3.11

### Patch Changes

- @pnpm/symlink-dependency@7.1.3
- @pnpm/store.cafs@2.0.10
- @pnpm/cafs-types@4.0.0
- @pnpm/fs.hard-link-dir@3.0.0
- @pnpm/create-cafs-store@6.0.10

## 0.3.10

### Patch Changes

- @pnpm/create-cafs-store@6.0.9

## 0.3.9

### Patch Changes

- 1e7bd4af3: Use availableParallelism, when available.

## 0.3.8

### Patch Changes

- @pnpm/store.cafs@2.0.9
- @pnpm/create-cafs-store@6.0.8
- @pnpm/fs.hard-link-dir@3.0.0
- @pnpm/symlink-dependency@7.1.2

## 0.3.7

### Patch Changes

- Updated dependencies [cfc017ee3]
  - @pnpm/create-cafs-store@6.0.7
  - @pnpm/store.cafs@2.0.8
  - @pnpm/fs.hard-link-dir@3.0.0
  - @pnpm/symlink-dependency@7.1.2

## 0.3.6

### Patch Changes

- 6390033cd: Directory hard linking moved to the worker.
- Updated dependencies [6390033cd]
  - @pnpm/fs.hard-link-dir@3.0.0
  - @pnpm/store.cafs@2.0.7
  - @pnpm/create-cafs-store@6.0.6
  - @pnpm/symlink-dependency@7.1.2
  - @pnpm/cafs-types@4.0.0

## 0.3.5

### Patch Changes

- @pnpm/create-cafs-store@6.0.5

## 0.3.4

### Patch Changes

- 08b65ff78: Update @rushstack/worker-pool.
- Updated dependencies [01bc58e2c]
  - @pnpm/store.cafs@2.0.6
  - @pnpm/create-cafs-store@6.0.4
  - @pnpm/symlink-dependency@7.1.1

## 0.3.3

### Patch Changes

- @pnpm/create-cafs-store@6.0.3

## 0.3.2

### Patch Changes

- @pnpm/create-cafs-store@6.0.2

## 0.3.1

### Patch Changes

- @pnpm/create-cafs-store@6.0.1
- @pnpm/symlink-dependency@7.1.1
- @pnpm/store.cafs@2.0.5
- @pnpm/cafs-types@4.0.0

## 0.3.0

### Minor Changes

- 9caa33d53: Remove `disableRelinkFromStore` and `relinkLocalDirDeps`. Replace them with `disableRelinkLocalDirDeps`.

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/create-cafs-store@6.0.0
  - @pnpm/cafs-types@4.0.0
  - @pnpm/graceful-fs@3.2.0
  - @pnpm/store.cafs@2.0.4
  - @pnpm/symlink-dependency@7.1.0

## 0.2.1

### Patch Changes

- @pnpm/create-cafs-store@5.1.1

## 0.2.0

### Minor Changes

- 48dcd108c: Improve performance of installation by using a worker for creating the symlinks inside `node_modules/.pnpm` [#7069](https://github.com/pnpm/pnpm/pull/7069).

### Patch Changes

- 03cdccc6e: New option added: disableRelinkFromStore.
- Updated dependencies [03cdccc6e]
- Updated dependencies [48dcd108c]
  - @pnpm/create-cafs-store@5.1.0
  - @pnpm/cafs-types@3.1.0
  - @pnpm/symlink-dependency@7.1.0
  - @pnpm/store.cafs@2.0.3

## 0.1.2

### Patch Changes

- Updated dependencies [b3947185c]
  - @pnpm/store.cafs@2.0.2
  - @pnpm/create-cafs-store@5.0.2

## 0.1.1

### Patch Changes

- Updated dependencies [b548f2f43]
- Updated dependencies [4a1a9431d]
  - @pnpm/store.cafs@2.0.1
  - @pnpm/cafs-types@3.0.1
  - @pnpm/create-cafs-store@5.0.1

## 0.1.0

### Minor Changes

- 083bbf590: Initial release.

### Patch Changes

- Updated dependencies [0fd9e6a6c]
- Updated dependencies [f2009d175]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
  - @pnpm/store.cafs@2.0.0
  - @pnpm/create-cafs-store@5.0.0
  - @pnpm/cafs-types@3.0.0
  - @pnpm/graceful-fs@3.1.0
