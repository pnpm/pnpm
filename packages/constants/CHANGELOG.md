# @pnpm/constants

## 1001.0.0

### Major Changes

- d2e83b0: Metadata directory version bumped to force fresh cache after we shipped a fix to the metadata write function. This change is backward compatible as install doesn't require a metadata cache.
- a76da0c: Removed lockfile conversion from v6 to v9. If you need to convert lockfile v6 to v9, use pnpm CLI v9.

## 10.0.0

### Major Changes

- 8108680: Changed the format of the side-effects cache key.
- c4f5231: Store version bumped to v10. The new store layout has a different directory called "index" for storing the package content mappings. Previously these files were stored in the same directory where the package contents are (in "files"). The new store has also a new format for storing the mappings for side-effects cache.

### Minor Changes

- 19d5b51: Add `MANIFEST_BASE_NAMES`

## 9.0.0

### Major Changes

- 83681da: Keep `libc` field in `clearMeta`.

## 8.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Minor Changes

- c692f80: Bump lockfile to v6.1

## 7.1.1

### Patch Changes

- 302ebffc5: Change lockfile version back to 6.0 as previous versions of pnpm fail to parse the version correctly.

## 7.1.0

### Minor Changes

- 9c4ae87bd: Bump lockfile v6 version to v6.1.

## 7.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 6.2.0

### Minor Changes

- 3ebce5db7: Exported a constant for the new lockfile format version: `LOCKFILE_FORMAT_V6`.

## 6.1.0

### Minor Changes

- 1267e4eff: Lockfile version bumped to 5.4

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

## 5.0.0

### Major Changes

- 6871d74b2: Bump lockfile version to 5.3.
- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- f2bb5cbeb: Bump layout version to 5.

## 4.1.0

### Minor Changes

- fcdad632f: Bump lockfile version to 5.2

## 4.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- ca9f50844: Bump lockfile version to 5.2.
- 4f5801b1c: Changing lockfile version back to 5.1

## 4.0.0-alpha.1

### Major Changes

- ca9f50844: Bump lockfile version to 5.2.

## 4.0.0-alpha.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
