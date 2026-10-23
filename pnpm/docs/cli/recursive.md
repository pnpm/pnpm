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

```sh
pnpm -r publish
```

By default, recursive `run`, `exec`, and `test` execute projects in
topological order: dependencies run before their dependents. Use `--no-sort`
to disable graph ordering, or `--parallel` to start tasks without waiting for
dependencies.

## Options

### --link-workspace-packages

Overrides the [`linkWorkspacePackages`](../workspaces.md#linkworkspacepackages) setting for this install.

### --workspace-concurrency

* Default: **the smaller of 4 and the number of available CPU cores**
* Type: **Number**

Set the maximum number of tasks to run simultaneously. For unlimited concurrency
use `Infinity`.

A value of `0` or less means the number of CPU cores minus its absolute value, and at least `1`. The matching setting is `workspaceConcurrency`.

### --[no-]bail

* Default: **true**
* Type: **Boolean**

If true, stops when a task throws an error.

`--no-bail` is accepted only by `run`, `exec`, `test`, `start`, `stop`, `rebuild`, `install-test`, and the `pnpm <script>` shorthand. Other commands reject it.

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

When `true`, commands follow the workspace dependency graph. Recursive `run`
also applies relationships from the [`tasks` setting]. Pass `--no-sort` to
remove graph ordering. This also makes `tasks`, `--reverse`, and `--resume-from`
inapplicable.

Example:
```sh
pnpm -r --no-sort test
```

### --reverse

* Default: **false**
* Type: **boolean**

When `true`, dependency edges are reversed. For recursive `run`, this reverses
the resolved task graph, including relationships declared with `dependsOn`.

```sh
pnpm -r --reverse run clean
```

### --parallel

See [`pnpm run --parallel`](./run.md#--parallel).

### --resume-from &lt;package_name\>

See [`pnpm run --resume-from`](./run.md#--resume-from-package_name).

### --stream

See [`pnpm run --stream`](./run.md#--stream).

### --aggregate-output

See [`pnpm run --aggregate-output`](./run.md#--aggregate-output).

### --report-summary

See [`pnpm run --report-summary`](./run.md#--report-summary).

### -w, --workspace-root

See [`--workspace-root`](../pnpm-cli.md#-w---workspace-root).

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

### --filter-prod &lt;package_selector\>

See [`--filter-prod`](../filtering.md#--filter-prod-filtering_pattern).

[includeWorkspaceRoot]: ../workspaces.md#includeworkspaceroot
[`tasks` setting]: ../workspace-task-orchestration.md
