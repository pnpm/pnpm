---
id: patch
title: "pnpm patch <pkg>"
---

Prepare a package for patching (inspired by a similar command in Yarn).

This command extracts the package to `node_modules/.pnpm_patches/<pkg>`, where `<pkg>` is the argument as given, such as `foo` or `foo@1.0.0` (or the [`--edit-dir`](#--edit-dir--d-dir)), a directory you can edit at will.

Once you're done with your changes, run `pnpm patch-commit <path>` (with `<path>` being the directory you received) to generate a patchfile and register it into your top-level manifest via the [`patchedDependencies`] field.

Usage:

```sh
pnpm patch <pkg name>@<version>
```

Run `pnpm install` first. Without installed dependencies, the command fails with `ERR_PNPM_PATCH_NO_LOCKFILE`.

If several versions of the package are installed, pnpm asks which version to patch, then whether to apply the patch to all versions. If you apply it to all versions, the [`patchedDependencies`] key is the package name without a version.

[`patchedDependencies`]: #patcheddependencies

:::note

To change a package's dependencies, don't use `pnpm patch` to modify its `package.json`! Use [overrides] or a [package hook] instead.

:::

[overrides]: ../settings/dependency-resolution.md#overrides
[package hook]: ../pnpmfile#hooksreadpackagepkg-context-pkg--promisepkg

<iframe width="560" height="315" src="https://www.youtube-nocookie.com/embed/0GjLqRGRbcY" title="The pnpm patch command demo" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen"></iframe>

## Options

### --edit-dir, -d &lt;dir>

The package that needs to be patched will be extracted to this directory. If the directory exists and is not empty, the command fails with `ERR_PNPM_PATCH_EDIT_DIR_EXISTS`. Without this option, a non-empty default directory fails with `ERR_PNPM_EDIT_DIR_NOT_EMPTY`.

### --ignore-existing

Ignore existing patch files when patching.

## Configuration

### patchedDependencies

This field is added/updated automatically when you run [pnpm patch-commit]. It defines patches for dependencies using a dictionary where:

[pnpm patch-commit]: ./patch-commit.md

* **Keys**: Package names with an exact version, a version range, or just the name.
* **Values**: Relative paths to patch files.

Example:

```yaml title="pnpm-workspace.yaml"
patchedDependencies:
  express@4.18.1: patches/express@4.18.1.patch
```

Dependencies can be patched by version range. The priority order is:

1. Exact versions (highest priority)
2. Version ranges
3. Name-only patches (applies to all versions unless overridden)

Example:

```yaml title="pnpm-workspace.yaml"
patchedDependencies:
  foo: patches/foo-1.patch
  foo@^2.0.0: patches/foo-2.patch
  foo@2.1.0: patches/foo-3.patch
```

* `patches/foo-3.patch` is applied to `foo@2.1.0`.
* `patches/foo-2.patch` applies to all foo versions matching `^2.0.0`, except `2.1.0`.
* `patches/foo-1.patch` applies to all other foo versions.

Avoid overlapping version ranges. If you need to specialize a sub-range, explicitly exclude it from the broader range.

Example:

```yaml title="pnpm-workspace.yaml"
patchedDependencies:
  # Specialized sub-range
  "foo@2.2.0-2.8.0": patches/foo.2.2.0-2.8.0.patch
  # General patch, excluding the sub-range above
  "foo@>=2.0.0 <2.2.0 || >2.8.0": patches/foo.gte2.patch
```

In most cases, defining an exact version is enough to override a broader range.

### allowUnusedPatches

* Default: **false**
* Type: **Boolean**

When `true`, installation won't fail if some of the patches from the `patchedDependencies` field were not applied.

```yaml title="pnpm-workspace.yaml"
patchedDependencies:
  express@4.18.1: patches/express@4.18.1.patch
allowUnusedPatches: true
```

A patch that fails to apply fails the install. When several patches in a group are applied, a failure in one does not prevent the rest from being attempted, and all patch errors are reported together at the end.
