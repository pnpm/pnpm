# @pnpm/store.cafs

## 1100.0.2

### Patch Changes

- @pnpm/fetching.fetcher-base@1100.0.2
- @pnpm/store.controller-types@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/fetching.fetcher-base@1100.0.1
  - @pnpm/store.controller-types@1100.0.1

## 1001.0.0

### Major Changes

- e2e0a32: Optimized index file format to store the hash algorithm once per file instead of repeating it for every file entry. Each file entry now stores only the hex digest instead of the full integrity string (`<algo>-<digest>`). Using hex format improves performance since file paths in the content-addressable store use hex representation, eliminating base64-to-hex conversion during path lookups.
- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- 56a59df: Store the bundled manifest (name, version, bin, engines, scripts, etc.) directly in the package index file, eliminating the need to read `package.json` from the content-addressable store during resolution and installation. This reduces I/O and speeds up repeat installs [#10473](https://github.com/pnpm/pnpm/pull/10473).

### Minor Changes

- 3bf5e21: Export a new function to add a new file to the CAFS.
- b7f0f21: Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).

### Patch Changes

- 6656baa: Fix a bug where the CAS locker cache was not updated when a file already existed with correct integrity, causing repeated integrity re-verification on subsequent lookups within the same process.
- 2ea6463: When pnpm installs a `file:` or `git:` dependency, it now validates that symlinks point within the package directory. Symlinks to paths outside the package root are skipped to prevent local data from being leaked into `node_modules`.

  This fixes a security issue where a malicious package could create symlinks to sensitive files (e.g., `/etc/passwd`, `~/.ssh/id_rsa`) and have their contents copied when the package is installed.

  Note: This only affects `file:` and `git:` dependencies. Registry packages (npm) have symlinks stripped during publish and are not affected.

- 50fbeca: fix: preserve bundled `node_modules` from Node.js Windows zip so that npm/npx shims are created correctly on Windows.

  The Windows Node.js distribution places npm inside a root-level `node_modules/` directory of the zip archive. `addFilesFromDir` was skipping root-level `node_modules` (to avoid treating a package's own npm dependencies as part of its content), which caused the bundled npm to be missing after installation. This prevented `pnpm env use` from creating the npm and npx shims on Windows.

  Added an `includeNodeModules` option to `addFilesFromDir` and set it to `true` in the binary fetcher so that the complete Node.js distribution, including its bundled npm, is preserved.

- caabba4: Fixed a path traversal vulnerability in tarball extraction on Windows. The path normalization was only checking for `./` but not `.\`. Since backslashes are directory separators on Windows, malicious packages could use paths like `foo\..\..\.npmrc` to write files outside the package directory.
- 878a773: Write CAS files directly to their final content-addressed path instead of writing to a temp file and renaming. Uses exclusive-create file mode for safe concurrent multi-process writes. Eliminates ~30k rename syscalls per cold install.
- f8e6774: Optimize hot path string operations in content-addressable store: replace `path.join` with string concatenation in `contentPathFromHex` and `getFilePathByModeInCafs` (~30k calls per install), and increase `gunzipSync` chunk size to 128KB for fewer buffer allocations during tarball decompression.
- Updated dependencies [facdd71]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [9d3f00b]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/fetching.fetcher-base@1002.0.0
  - @pnpm/fs.graceful-fs@1001.0.0
  - @pnpm/error@1001.0.0

## 1000.0.19

### Patch Changes

- Updated dependencies [7c1382f]
  - @pnpm/store-controller-types@1004.1.0
  - @pnpm/fetcher-base@1001.0.2

## 1000.0.18

### Patch Changes

- 9b9faa5: Retry filesystem operations on EAGAIN errors [#9959](https://github.com/pnpm/pnpm/pull/9959).
- Updated dependencies [9b9faa5]
  - @pnpm/graceful-fs@1000.0.1

## 1000.0.17

### Patch Changes

- @pnpm/fetcher-base@1001.0.1
- @pnpm/store-controller-types@1004.0.2

## 1000.0.16

### Patch Changes

- Updated dependencies [d1edf73]
  - @pnpm/fetcher-base@1001.0.0
  - @pnpm/store-controller-types@1004.0.1

## 1000.0.15

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/store-controller-types@1004.0.0
  - @pnpm/fetcher-base@1000.1.0

## 1000.0.14

### Patch Changes

- @pnpm/fetcher-base@1000.0.12
- @pnpm/store-controller-types@1003.0.3

## 1000.0.13

### Patch Changes

- Updated dependencies [509948d]
  - @pnpm/store-controller-types@1003.0.2

## 1000.0.12

### Patch Changes

- Updated dependencies [c24c66e]
  - @pnpm/store-controller-types@1003.0.1
  - @pnpm/fetcher-base@1000.0.11

## 1000.0.11

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
  - @pnpm/store-controller-types@1003.0.0
  - @pnpm/fetcher-base@1000.0.10

## 1000.0.10

### Patch Changes

- @pnpm/fetcher-base@1000.0.9
- @pnpm/store-controller-types@1002.0.1

## 1000.0.9

### Patch Changes

- Updated dependencies [72cff38]
  - @pnpm/store-controller-types@1002.0.0
  - @pnpm/fetcher-base@1000.0.8

## 1000.0.8

### Patch Changes

- @pnpm/fetcher-base@1000.0.7
- @pnpm/store-controller-types@1001.0.5

## 1000.0.7

### Patch Changes

- @pnpm/fetcher-base@1000.0.6
- @pnpm/store-controller-types@1001.0.4

## 1000.0.6

### Patch Changes

- @pnpm/fetcher-base@1000.0.5
- @pnpm/store-controller-types@1001.0.3

## 1000.0.5

### Patch Changes

- @pnpm/fetcher-base@1000.0.4
- @pnpm/store-controller-types@1001.0.2

## 1000.0.4

### Patch Changes

- @pnpm/fetcher-base@1000.0.3
- @pnpm/store-controller-types@1001.0.1

## 1000.0.3

### Patch Changes

- Updated dependencies [dde650b]
  - @pnpm/store-controller-types@1001.0.0

## 1000.0.2

### Patch Changes

- @pnpm/fetcher-base@1000.0.2
- @pnpm/store-controller-types@1000.1.1

## 1000.0.1

### Patch Changes

- Updated dependencies [6483b64]
  - @pnpm/store-controller-types@1000.1.0
  - @pnpm/fetcher-base@1000.0.1

## 5.0.0

### Major Changes

- d433cb9: Some registries allow identical content to be published under different package names or versions. To accommodate this, index files in the store are now stored using both the content hash and package identifier.

  This approach ensures that we can:

  1. Validate that the integrity in the lockfile corresponds to the correct package,
     which might not be the case after a poorly resolved Git conflict.
  2. Allow the same content to be referenced by different packages or different versions of the same package.

  Related PR: [#8510](https://github.com/pnpm/pnpm/pull/8510)
  Related issue: [#8204](https://github.com/pnpm/pnpm/issues/8204)

- 099e6af: Changed the structure of the index files in the store to store side effects cache information more efficiently. In the new version, side effects do not list all the files of the package but just the differences [#8636](https://github.com/pnpm/pnpm/pull/8636).

### Patch Changes

- @pnpm/fetcher-base@16.0.7
- @pnpm/store-controller-types@18.1.6

## 4.0.2

### Patch Changes

- a1f4df2: Fixed a race condition in temporary file creation in the store by including worker thread ID in filename. Previously, multiple worker threads could attempt to use the same temporary file. Temporary files now include both process ID and thread ID for uniqueness [#8703](https://github.com/pnpm/pnpm/pull/8703).

## 4.0.1

### Patch Changes

- db7ff76: When checking whether a file in the store has executable permissions, the new approach checks if at least one of the executable bits (owner, group, and others) is set to 1. Previously, a file was incorrectly considered executable only when all the executable bits were set to 1. This fix ensures that files with any executable permission, regardless of the user class, are now correctly identified as executable [#8546](https://github.com/pnpm/pnpm/issues/8546).

## 4.0.0

### Major Changes

- db420ab: `getFilePathInCafs` renamed to `getIndexFilePathInCafs`.

### Patch Changes

- @pnpm/fetcher-base@16.0.7
- @pnpm/store-controller-types@18.1.6

## 3.0.8

### Patch Changes

- @pnpm/fetcher-base@16.0.6
- @pnpm/store-controller-types@18.1.5

## 3.0.7

### Patch Changes

- @pnpm/fetcher-base@16.0.5
- @pnpm/store-controller-types@18.1.4

## 3.0.6

### Patch Changes

- @pnpm/fetcher-base@16.0.4
- @pnpm/store-controller-types@18.1.3

## 3.0.5

### Patch Changes

- afe520d: Update rename-overwrite to v6.
- afe520d: Update symlink-dir to v6.0.1.

## 3.0.4

### Patch Changes

- @pnpm/fetcher-base@16.0.3
- @pnpm/store-controller-types@18.1.2

## 3.0.3

### Patch Changes

- @pnpm/fetcher-base@16.0.2
- @pnpm/store-controller-types@18.1.1

## 3.0.2

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-controller-types@18.1.0

## 3.0.1

### Patch Changes

- @pnpm/fetcher-base@16.0.1
- @pnpm/store-controller-types@18.0.1

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- 36dcaa0: Breaking change to addFileFromDir args.

### Minor Changes

- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- 6cdbf11: Don't fail on a tarball that appears to be not a USTAR or GNU TAR archive. Still try to unpack the tarball [#7120](https://github.com/pnpm/pnpm/issues/7120).
- Updated dependencies [43cdd87]
- Updated dependencies [730929e]
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/fetcher-base@16.0.0
  - @pnpm/graceful-fs@4.0.0

## 2.0.12

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/store-controller-types@17.2.0
  - @pnpm/fetcher-base@15.0.7

## 2.0.11

### Patch Changes

- 33313d2fd: Update rename-overwrite to v5.
  - @pnpm/fetcher-base@15.0.6
  - @pnpm/store-controller-types@17.1.4

## 2.0.10

### Patch Changes

- @pnpm/fetcher-base@15.0.5
- @pnpm/store-controller-types@17.1.3

## 2.0.9

### Patch Changes

- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2

## 2.0.8

### Patch Changes

- Updated dependencies [7ea45afbe]
  - @pnpm/store-controller-types@17.1.1
  - @pnpm/fetcher-base@15.0.4

## 2.0.7

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/fetcher-base@15.0.3

## 2.0.6

### Patch Changes

- 01bc58e2c: Update ssri to v10.0.5.

## 2.0.5

### Patch Changes

- @pnpm/fetcher-base@15.0.2
- @pnpm/store-controller-types@17.0.1

## 2.0.4

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0
  - @pnpm/graceful-fs@3.2.0
  - @pnpm/fetcher-base@15.0.1

## 2.0.3

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0
  - @pnpm/fetcher-base@15.0.1

## 2.0.2

### Patch Changes

- b3947185c: Tarballs that have hard links are now unpacked successfully. This fixes a regression introduced in v8.7.0, which was shipped with our new in-house tarball parser [#7062](https://github.com/pnpm/pnpm/pull/7062).

## 2.0.1

### Patch Changes

- b548f2f43: Fixes a regression published with pnpm v8.7.3. Don't hang while reading `package.json` from the content-addressable store [#7051](https://github.com/pnpm/pnpm/pull/7051).
- Updated dependencies [4a1a9431d]
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/store-controller-types@16.0.1

## 2.0.0

### Major Changes

- 083bbf590: Breaking changes to the API.

### Patch Changes

- 0fd9e6a6c: Don't prematurely bail out of adding source files if ENOENT is thrown [#6932](https://github.com/pnpm/pnpm/pull/6932).
- Updated dependencies [494f87544]
- Updated dependencies [70b2830ac]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/fetcher-base@15.0.0
  - @pnpm/graceful-fs@3.1.0

## 1.0.2

### Patch Changes

- 73f2b6826: When several containers use the same store simultaneously, there's a chance that multiple containers may create a temporary file at the same time. In such scenarios, pnpm could fail to rename the temporary file in one of the containers. This issue has been addressed: pnpm will no longer fail if the temporary file is absent but the destination file exists.

## 1.0.1

### Patch Changes

- fe1c5f48d: The length of the temporary file names in the content-addressable store reduced in order to prevent `ENAMETOOLONG` errors from happening [#6842](https://github.com/pnpm/pnpm/issues/6842).

## 1.0.0

### Major Changes

- 4bbf482d1: The package is renamed from `@pnpm/cafs` to `@pnpm/store.cafs`.

  The content-addressable store locker should be only created once per process. This fixes an issue that started happening after merging [#6817](https://github.com/pnpm/pnpm/pull/6817)
