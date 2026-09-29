---
id: recursive
title: pnpm recursive
original_id: recursive
---

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm install -r` | runs installation for every package in every subfolder |
|`pnpm run build --filter foo-*` |builds all packages with names that start with `foo-` |
|`pnpm update -- login-page...` |updates dependencies in `login-page` and any dependencies of `login-page` that are also in the repository |

## Options

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)

### workspace-concurrency

Added in: v2.13.0

* Default: **4**
* Type: **Number**

Set the maximum number of concurrency. For unlimited concurrency use `Infinity`.

### bail

Added in: v2.13.0

* Default: **true**
* Type: **Boolean**

If true, stops when a task throws an error.

This config does not affect the exit code.
Even if `--no-bail` is used, all tasks will finish but if any of the tasks fail, the
command will exit with a non-zero code.

Usage example. Run tests in every package. Continue if tests fail in one of the packages:

```
pnpm recursive test --no-bail
```

### sort

Added in: v2.14.0

* Default: **true**
* Type: **Boolean**

When `true`, packages are sorted topologically (dependencies before dependents). Pass `--no-sort` to disable.

Usage examples:

```sh
pnpm recursive test --no-sort
```

## pnpm recursive exec

Added in: v2.9.0

```test
pnpm recursive exec -- <command> [args...]
```

This command runs a command in each package of the multi-package repo.

The name of the current package is available through the environment variable `PNPM_PACKAGE_NAME` (supported from pnpm v2.22.0).

Usage examples:

```sh
pnpm recursive exec -- rm -rf node_modules
pnpm recursive exec -- pnpm view \$PNPM_PACKAGE_NAME
```
