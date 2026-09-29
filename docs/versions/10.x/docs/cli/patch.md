---
id: patch
title: "pnpm patch <pkg>"
---

Prepare a package for patching (inspired by a similar command in Yarn).

This command will cause a package to be extracted in a temporary directory intended to be editable at will.

Once you're done with your changes, run `pnpm patch-commit <path>` (with `<path>` being the temporary directory you received) to generate a patchfile and register it into your top-level manifest via the [`patchedDependencies`] field.

Usage:

```
pnpm patch <pkg name>@<version>
```

[`patchedDependencies`]: #patcheddependencies

:::note

If you want to change the dependencies of a package, don't use patching to modify the `package.json` file of the package. For overriding dependencies, use [overrides] or a [package hook].

:::

[overrides]: ../settings.md#overrides
[package hook]: ../pnpmfile#hooksreadpackagepkg-context-pkg--promisepkg

<iframe width="560" height="315" src="https://www.youtube-nocookie.com/embed/0GjLqRGRbcY" title="The pnpm patch command demo" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen"></iframe>

## Options

### --edit-dir &lt;dir>

The package that needs to be patched will be extracted to this directory.

### --ignore-existing

Ignore existing patch files when patching.

## Configuration

### patchedDependencies

This field is added/updated automatically when you run [pnpm patch-commit]. It defines patches for dependencies using a dictionary where:

[pnpm patch-commit]: ./patch-commit.md

* **Keys**: Package names with an exact version, a version range, or just the name.
* **Values**: Relative paths to patch files.

Example:

```yaml
patchedDependencies:
  express@4.18.1: patches/express@4.18.1.patch
```

Dependencies can be patched by version range. The priority order is:

1. Exact versions (highest priority)
2. Version ranges
3. Name-only patches (applies to all versions unless overridden)

A special case: the version range `*` behaves like a name-only patch but does not ignore patch failures.

Example:

```yaml
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

```yaml
patchedDependencies:
  # Specialized sub-range
  "foo@2.2.0-2.8.0": patches/foo.2.2.0-2.8.0.patch
  # General patch, excluding the sub-range above
  "foo@>=2.0.0 <2.2.0 || >2.8.0": patches/foo.gte2.patch
```

In most cases, defining an exact version is enough to override a broader range.

### allowUnusedPatches

Added in: v10.7.0 (Previously named `allowNonAppliedPatches`)

* Default: **false**
* Type: **Boolean**

When `true`, installation won't fail if some of the patches from the `patchedDependencies` field were not applied.

```json
patchedDependencies:
  express@4.18.1: patches/express@4.18.1.patch
allowUnusedPatches: true
```

### ignorePatchFailures

Added in: v10.7.0

* Default: **undefined**
* Type: **Boolean**, **undefined**

Controls how patch failures are handled.

Behaviour:

* **undefined (default)**:
  * Errors out when a patch with an exact version or version range fails.
  * Ignores failures from name-only patches.
* **false**: Errors out for any patch failure.
* **true**: Prints a warning instead of failing when any patch cannot be applied.
