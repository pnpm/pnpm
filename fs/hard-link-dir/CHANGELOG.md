# @pnpm/fs.hard-link-dir

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
