---
id: tasks
title: pnpm tasks
---

Added in: v12.6.0

Inspect tasks in [concurrency groups](../workspace-task-orchestration.md#concurrencygroups).

:::note

If your package or workspace defines a script named `"tasks"` in `package.json`, that script takes precedence when running `pnpm tasks`. Use [`pnpm pm tasks`](./pm.md) to force the built-in command.

:::

## Commands

### status

Show running and waiting tasks in [concurrency groups](../workspace-task-orchestration.md#concurrencygroups).

```sh
pnpm tasks status [groups...]
```

Lists the running (active holders) and waiting tasks across [concurrency groups](../workspace-task-orchestration.md#concurrencygroups), including how long each task has been running or waiting, its process ID, working directory, and its [task priority](../workspace-task-orchestration.md#task-priority).

#### Examples

Show all active concurrency groups used by pnpm processes sharing the same [`stateDir`](../settings/other.md#statedir):

```sh
pnpm tasks status
```

If no concurrency groups are currently in use, pnpm outputs:

```
No concurrency groups are in use.
```

Inspect specific concurrency groups:

```sh
pnpm tasks status cargo typescript
```

When group names are specified, pnpm outputs the status of only those groups, including idle groups:

```
cargo
  running
    build:native  12s
      pid 45123 in /workspace/crates/native
  waiting
    1. test:native  3s  priority 2
      pid 45201 in /workspace/crates/native
    2. bench:native  1s  priority 0
      pid 45210 in /workspace/crates/native

typescript: idle
```

#### Script precedence

If your project defines a `"tasks"` script in `package.json`:

```sh
# Runs the "tasks" script from package.json
pnpm tasks status

# Runs the built-in pnpm tasks status command
pnpm pm tasks status
```

## See also

* [Workspace task orchestration](../workspace-task-orchestration.md#concurrency-groups)
* [Task priority](../workspace-task-orchestration.md#task-priority)
* [pnpm pm](./pm.md)
