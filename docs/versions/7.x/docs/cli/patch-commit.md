---
id: patch-commit
title: "pnpm patch-commit <path>"
---

Added in: v7.4.0

Generate a patch out of a directory (inspired by a similar command in Yarn).

```sh
pnpm patch-commit <patchDir>
```

## Options

### ---patches-dir &lt;dir>

Added in: v7.30.0

The generated patch file will be saved to this directory. By default, patches are saved to the `patches` directory in the root of the project.
