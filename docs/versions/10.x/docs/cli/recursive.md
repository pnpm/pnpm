---
id: recursive
title: pnpm -r, --recursive
---

Aliases: `m`, `multi`, `recursive`, `<command> -r`

Runs a command in every project of a workspace, when used with the following commands:

* `install`
* `list`
* `outdated`
* `publish`
* `pack`
* `rebuild`
* `remove`
* `unlink`
* `update`
* `why`

Runs a command in every project of a workspace, excluding the root project,
when used with the following commands:

* `exec`
* `run`
* `test`
* `add`

If you want the root project be included even when running scripts, set the [includeWorkspaceRoot] setting to `true`.

Usage example:

```
pnpm -r publish
```

## Options

### --link-workspace-packages

* Default: **false**
* Type: **true, false, deep**

Link locally available packages in workspaces of a monorepo into `node_modules`
instead of re-downloading them from the registry. This emulates functionality
similar to `yarn workspaces`.

When this is set to deep, local packages can also be linked to subdependencies.

Be advised that it is encouraged instead to use [`pnpm-workspace.yaml`] for this setting, to
enforce the same behaviour in all environments. This option exists solely so you
may override that if necessary.

[`pnpm-workspace.yaml`]: ../settings.md#linkWorkspacePackages

### --workspace-concurrency

* Default: **4**
* Type: **Number**

Set the maximum number of tasks to run simultaneously. For unlimited concurrency
use `Infinity`.

You can set the `workspace-concurrency` as `<= 0` and it will use amount of cores of the host as: `max(1, (number of cores) - abs(workspace-concurrency))`

### --[no-]bail

* Default: **true**
* Type: **Boolean**

If true, stops when a task throws an error.

This config does not affect the exit code.
Even if `--no-bail` is used, all tasks will finish but if any of the tasks fail,
the command will exit with a non-zero code.

Example (run tests in every package, continue if tests fail in one of them):
```sh
pnpm -r --no-bail test
```

### --[no-]sort

* Default: **true**
* Type: **Boolean**

When `true`, packages are sorted topologically (dependencies before dependents).
Pass `--no-sort` to disable.

Example:
```sh
pnpm -r --no-sort test
```

### --reverse

* Default: **false**
* Type: **boolean**

When `true`, the order of packages is reversed.

```
pnpm -r --reverse run clean
```

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

[includeWorkspaceRoot]: ../settings.md#includeWorkspaceRoot
