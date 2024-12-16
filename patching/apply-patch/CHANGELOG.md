# @pnpm/patching.apply-patch

## 1000.0.1

### Patch Changes

- @pnpm/error@1000.0.1

## 3.1.2

### Patch Changes

- @pnpm/error@6.0.3

## 3.1.1

### Patch Changes

- @pnpm/error@6.0.2

## 3.1.0

### Minor Changes

- cb006df: Add ability to apply patch to all versions:
  If the key of `pnpm.patchedDependencies` is a package name without a version (e.g. `pkg`), pnpm will attempt to apply the patch to all versions of
  the package, failure will be skipped.
  If it is a package name and an exact version (e.g. `pkg@x.y.z`), pnpm will attempt to apply the patch to that exact version only, failure will
  cause pnpm to fail.

  If there's only one version of `pkg` installed, `pnpm patch pkg` and subsequent `pnpm patch-commit $edit_dir` will create an entry named `pkg` in
  `pnpm.patchedDependencies`. And pnpm will attempt to apply this patch to other versions of `pkg` in the future.

  If there's multiple versions of `pkg` installed, `pnpm patch pkg` will ask which version to edit and whether to attempt to apply the patch to all.
  If the user chooses to apply the patch to all, `pnpm patch-commit $edit_dir` would create a `pkg` entry in `pnpm.patchedDependencies`.
  If the user chooses not to apply the patch to all, `pnpm patch-commit $edit_dir` would create a `pkg@x.y.z` entry in `pnpm.patchedDependencies` with
  `x.y.z` being the version the user chose to edit.

  If the user runs `pnpm patch pkg@x.y.z` with `x.y.z` being the exact version of `pkg` that has been installed, `pnpm patch-commit $edit_dir` will always
  create a `pkg@x.y.z` entry in `pnpm.patchedDependencies`.

## 3.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
  - @pnpm/error@6.0.0

## 2.0.5

### Patch Changes

- 512d71254: `pnpm patch` should write patch files with a trailing newline [#6905](https://github.com/pnpm/pnpm/pull/6905).

## 2.0.4

### Patch Changes

- 3b6930263: Throw a meaningful error when applying a patch to a dependency fails.

## 2.0.3

### Patch Changes

- @pnpm/error@5.0.2

## 2.0.2

### Patch Changes

- 47f529ebf: Update patch-package.

## 2.0.1

### Patch Changes

- @pnpm/error@5.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/error@5.0.0

## 1.0.0

### Major Changes

- 2ae1c449d: Initial release.
