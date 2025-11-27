# @pnpm/fs.hard-link-dir

## 1000.0.5

### Patch Changes

- 66b2c58: Handle ENOENT errors thrown by `fs.linkSync()`, which can occur in containerized environments (OverlayFS) instead of EXDEV. The operation now gracefully falls back to `fs.copyFileSync()` in these cases [#10217](https://github.com/pnpm/pnpm/issues/10217).

## 1000.0.4

### Patch Changes

- 4a9422d: Don't crash when two processes of pnpm are hardlinking the contents of a directory to the same destination simultaneously [#10179](https://github.com/pnpm/pnpm/issues/10179).

## 1000.0.3

### Patch Changes

- a7cf087: Don't crash when two processes of pnpm are hardlinking the contents of a directory to the same destination simultaneously [#10160](https://github.com/pnpm/pnpm/pull/10160).

## 1000.0.2

### Patch Changes

- 9b9faa5: Retry filesystem operations on EAGAIN errors [#9959](https://github.com/pnpm/pnpm/pull/9959).
- Updated dependencies [9b9faa5]
  - @pnpm/graceful-fs@1000.0.1

## 1000.0.1

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.

## 4.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

## 3.0.0

### Major Changes

- 6390033cd: Changed to be sync.

## 2.0.1

### Patch Changes

- 64d0f47ff: Warn user when `publishConfig.directory` of an injected workspace dependency does not exist [#6396](https://github.com/pnpm/pnpm/pull/6396).

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 1.0.3

### Patch Changes

- 6add01403: Ignore broken symlinks.
- 5c4eb0fc3: Performance optimization.

## 1.0.2

### Patch Changes

- 78d4cf1f7: Fall back to copying files if creating hard links fails with cross-device linking error [#5992](https://github.com/pnpm/pnpm/issues/5992).

## 1.0.1

### Patch Changes

- 00d86db16: Don't try to hard link the source directory to itself.

## 1.0.0

### Major Changes

- c9d3970e3: Initial release.
