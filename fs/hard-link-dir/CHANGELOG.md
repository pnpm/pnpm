# @pnpm/fs.hard-link-dir

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
