---
id: patch
title: "pnpm patch <pkg>"
---

Added in: v7.4.0

Prepare a package for patching (inspired by a similar command in Yarn).

This command will cause a package to be extracted in a temporary directory intended to be editable at will.

Once you're done with your changes, run `pnpm patch-commit <path>` (with `<path>` being the temporary directory you received) to generate a patchfile and register it into your top-level manifest via the [`patchedDependencies`] field.

Usage:

```
pnpm patch <pkg name>@<version>
```

[`patchedDependencies`]: ../package_json.md#pnpmpatcheddependencies

<iframe width="560" height="315" src="https://www.youtube-nocookie.com/embed/0GjLqRGRbcY" title="The pnpm patch command demo" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen"></iframe>

## Options

### --edit-dir &lt;dir>

Added in: v7.11.0

The package that needs to be patched will be extracted to this directory.

### --ignore-existing

Added in: v7.25.0

Ignore existing patch files when patching.
