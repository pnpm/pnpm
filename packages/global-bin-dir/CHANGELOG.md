# @pnpm/global-bin-dir

## 1.2.5

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0

## 1.2.4

### Patch Changes

- 846887de3: When searching for a suitable global bin directory, search for symlinked node, npm, pnpm commands, not only command files.

## 1.2.3

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1

## 1.2.2

### Patch Changes

- 4d4d22b63: A directory is considered a valid global executable directory for pnpm, if it contains a node, or npm, or pnpm executable, not directory.

## 1.2.1

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0

## 1.2.0

### Minor Changes

- ad69677a7: Add a new optional argument. When the argument is `false`, a global bin directory is returned even if the process has no write access to it.

## 1.1.1

### Patch Changes

- 245221baa: When searching a suitable global executables directory, take any directory from the PATH that has a node, pnpm, or npm command in it.

## 1.1.0

### Minor Changes

- 915828b46: `globalBinDir()` may accept an array of suitable executable directories.
  If one of these directories is in PATH and has bigger priority than the
  npm/pnpm/nodejs directories, then that directory will be used.

## 1.0.1

### Patch Changes

- 2c190d49d: When looking for suitable directories for global executables, ignore case.

  When comparing to the currently running Node.js executable directory,
  ignore any trailing slash. `/foo/bar` is the same as `/foo/bar/`.

## 1.0.0

### Major Changes

- 1146b76d2: Finds a directory that is in PATH and we have permission to write to it.
