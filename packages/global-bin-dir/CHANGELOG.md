# @pnpm/global-bin-dir

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
