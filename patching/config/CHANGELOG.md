# @pnpm/patching.config

## 1001.0.0

### Major Changes

- 5f7be64: Add an ability to patch dependencies by version ranges. Exact versions override version ranges, which in turn override name-only patches. Version range `*` is the same as name-only, except that patch application failure will not be ignored.

  For example:

  ```yaml
  patchedDependencies:
    foo: patches/foo-1.patch
    foo@^2.0.0: patches/foo-2.patch
    foo@2.1.0: patches/foo-3.patch
  ```

  The above configuration would apply `patches/foo-3.patch` to `foo@2.1.0`, `patches/foo-2.patch` to all `foo` versions which satisfy `^2.0.0` except `2.1.0`, and `patches/foo-1.patch` to the remaining `foo` versions.

  > [!WARNING]
  > The version ranges should not overlap. If you want to specialize a sub range, make sure to exclude it from the other keys. For example:
  >
  > ```yaml
  > # pnpm-workspace.yaml
  > patchedDependencies:
  >   # the specialized sub range
  >   'foo@2.2.0-2.8.0': patches/foo.2.2.0-2.8.0.patch
  >   # the more general patch, excluding the sub range above
  >   'foo@>=2.0.0 <2.2.0 || >2.8.0': 'patches/foo.gte2.patch
  > ```
  >
  > In most cases, however, it's sufficient to just define an exact version to override the range.

### Minor Changes

- 5f7be64: Rename `pnpm.allowNonAppliedPatches` to `pnpm.allowUnusedPatches`. The old name is still supported but it would print a deprecation warning message.
- 5f7be64: Add `pnpm.ignorePatchFailures` to manage whether pnpm would ignore patch application failures.

  If `ignorePatchFailures` is not set, pnpm would throw an error when patches with exact versions or version ranges fail to apply, and it would ignore failures from name-only patches.

  If `ignorePatchFailures` is explicitly set to `false`, pnpm would throw an error when any type of patch fails to apply.

  If `ignorePatchFailures` is explicitly set to `true`, pnpm would print a warning when any type of patch fails to apply.

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/patching.types@1000.1.0
  - @pnpm/dependency-path@1000.0.6

## 1.0.0

### Major Changes

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

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/patching.types@1.0.0
