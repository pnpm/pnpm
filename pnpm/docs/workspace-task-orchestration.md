---
id: workspace-task-orchestration
title: Workspace task orchestration
---

`pnpm -r run <script>` schedules a graph of workspace tasks. A task is a script
in one workspace project, identified as `<project>#<script>`. It becomes ready
after every task it depends on completes successfully, and ready tasks run under
the [`--workspace-concurrency`](./cli/recursive.md#--workspace-concurrency)
limit.

Independent tasks do not wait for an unrelated project to finish. Their start
and completion order is therefore not guaranteed.

## Configure task dependencies

Declare task relationships under `tasks` in `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
packages:
  - packages/*

tasks:
  build:
    dependsOn:
      - ^build
  test:
    dependsOn:
      - build
```

Each `dependsOn` entry has one of these forms:

| Entry | Meaning |
| --- | --- |
| `build` | The `build` task in the same project |
| `^build` | The `build` task in each selected workspace dependency of the project |

In this example, `pnpm -r run test` first runs `build` in each project. A
project's `build` waits for `build` in its workspace dependencies.

A task with no entry under `tasks` defaults to depending on the same task in
its workspace dependencies. For example, an unconfigured `build` behaves as
`dependsOn: ['^build']`, preserving the usual dependencies-before-dependents
behavior.

:::warning

Once a task has an entry under `tasks`, an omitted `dependsOn` is the same as
`dependsOn: []`. If you configure another field and still want the default
topological relationship, declare it explicitly:

```yaml title="pnpm-workspace.yaml"
tasks:
  build:
    concurrency: 2
    dependsOn:
      - ^build
```

:::

Task dependencies stay inside the projects selected by `--filter` and
`includeWorkspaceRoot`. A `dependsOn` entry does not widen that selection.

### Projects without a script

If a selected project does not have a script named by the graph, pnpm treats
that task as a pass-through. The task is reported as skipped after its own
dependencies complete, so a package without `build` does not sever the build
chain between its workspace dependencies and dependents.

## Limit concurrency for one task

Set `concurrency` to a positive integer to limit how many instances of a named
task may run across workspace projects at once:

```yaml title="pnpm-workspace.yaml"
tasks:
  build:
    concurrency: 2
    dependsOn:
      - ^build
```

This limit is separate from the workspace-wide
[`--workspace-concurrency`](./cli/recursive.md#--workspace-concurrency) limit.
A `build` waiting for one of its two slots does not occupy a workspace slot, so
an unrelated ready task can still run.

## concurrencyGroups

Added in: v12.5.0

Assign tasks to a `concurrencyGroup`, then give the group a machine-wide limit:

```yaml title="pnpm-workspace.yaml"
tasks:
  test:rust:
    concurrencyGroup: cargo
    dependsOn: []
concurrencyGroups:
  cargo: 2
```

At most two tasks in `cargo` run at once across pnpm processes sharing the same [`stateDir`](./settings/other.md#statedir), including `pnpm pipeline`. Tasks wait when every slot is held. A nested `pnpm run` in the same group uses its parent's slot, avoiding a wait on itself. Slots are released when the process ends, including crashes.

Changing `stateDir` creates a separate slot pool, so processes using different directories do not share the limit.

Group limits supplement task `concurrency` and `--workspace-concurrency`. A missing or zero group limit does not restrict execution. Use the same limit in workspaces sharing a group: each process honors its own configured limit.

### Task priority

Added in: v12.6.0

* Default: **0**
* Type: **Integer**

Waiting tasks take available slots in order of descending `priority`, with higher values running first. When `priority` is not specified, it defaults to `0`. A task with `priority: 10` therefore has higher priority than an unconfigured task and runs before it, while negative priorities run after. When two tasks have the same priority, arrival order breaks the tie.

```yaml title="pnpm-workspace.yaml"
tasks:
  build:critical:
    concurrencyGroup: build
    priority: 10
  build:docs:
    concurrencyGroup: build
    priority: 1
  build:cleanup:
    concurrencyGroup: build
    priority: -1
```

### `pnpm tasks status`

Added in: v12.6.0

[`pnpm tasks status`](./cli/tasks.md) lists running and waiting tasks in each concurrency group:

```sh
pnpm tasks status
```

If your workspace has a package script named `tasks`, that script takes precedence. In that case, use `pnpm pm tasks status` instead.

## Inspect the task graph

Use `--dry-run` to resolve the graph without running scripts:

```sh
pnpm -r run --dry-run build
```

The output is one stable topological ordering, with ties broken by project
directory. It is not a prediction of dispatch order: independent tasks may run
in any order.

Add `--json` to receive graph nodes and edges:

```sh
pnpm -r run --dry-run --json test
```

```json
{
  "tasks": [
    {
      "project": "packages/app",
      "script": "build",
      "missingScript": false,
      "dependsOn": [
        { "project": "packages/lib", "script": "build" }
      ]
    },
    {
      "project": "packages/app",
      "script": "test",
      "missingScript": false,
      "dependsOn": [
        { "project": "packages/app", "script": "build" }
      ]
    }
  ]
}
```

`project` is relative to the workspace root. The `tasks` array and each
`dependsOn` array are sorted by project and script so the output is stable.

## Cycles

pnpm checks the graph after project selection and task expansion. A cycle fails
before any script starts with `ERR_PNPM_TASK_CYCLE`, and the error names the
participating tasks.

Set [`ignoreWorkspaceCycles`](./workspaces.md#ignoreworkspacecycles) to `true`
only when the cycle is deliberate. pnpm then warns, removes the ordering among
the cycle's members, and may run them in any order relative to each other.

## Recursive run options

### `--resume-from <package_name>`

The named package's requested task is the resume point.

pnpm records which tasks pass as a recursive `run` or `exec` proceeds. When that
record belongs to the same invocation — the same selected projects, command,
arguments, and execution-affecting settings (including script bodies for
`run`) — `--resume-from` skips exactly the tasks the record says passed,
wherever they sit in the graph, and runs everything else. A record left by a
different invocation is ignored rather than trusted.

Without a usable record — a first run, a run interrupted before any task
passed, or a `node_modules` directory pnpm cannot write to — pnpm falls back to
graph position: it omits the resume point's transitive dependencies, treating
them as already completed, but still runs the resume task, its dependents, and
unrelated tasks in the selected graph. That assumption holds after a failed run
and not after a cancelled one, where a dependency may never have started.

### `--reverse`

pnpm reverses every edge in the resolved task graph, including relationships
declared with `dependsOn`. Tasks that normally depend on another task run before
it.

### `--no-bail`

After a task fails, tasks that depend on it are skipped. With `--no-bail`,
independent ready tasks continue to run and the command exits with a non-zero
code after they settle.

With the default `--bail`, pnpm stops dispatching new tasks after the first
failure and cancels the tasks already running, along with the processes they
started. Otherwise a task that never exits on its own, such as a watcher or a
dev server, would keep a failed run alive indefinitely. A cancelled task is not
reported as a failure of its own; the failure that stopped the run is the one
reported.

### Output

pnpm inherits a script's output directly when at most one script can be running
at any time, either because workspace concurrency is `1` or because the graph
forms one serial chain. When scripts can overlap, pnpm pipes their output so it
can prefix or aggregate it. Use [`--stream`](./cli/run.md#--stream) for immediate
prefixed output or [`--aggregate-output`](./cli/run.md#--aggregate-output) to
print each task's output together after it finishes.

## Caching a task

The `outputs`, `inputs`, `env`, `cache`, and `cargoTargetDir` keys of a task are
read by [`pnpm pipeline`](./cli/pipeline.md#caching-a-task) only. A recursive
`pnpm run` never restores a task from the cache.

## Commands that do not use `tasks`

The `tasks` declarations configure recursive `run`. Recursive `exec` follows
workspace project dependencies but has no script task name, so it does not join
declared `dependsOn` relationships. It still uses the dependency-aware
scheduler and the persisted `--resume-from` state described above.

`--no-sort` removes graph ordering, and `--parallel` implies `--no-sort`.
Consequently, both options ignore `tasks` declarations; `--reverse` and
`--resume-from` also have no ordering edges to transform.

## Other dependency-aware workspace commands

Workspace install, rebuild, pack, publish, stage, and lifecycle work uses the
same ready-queue scheduling principle: work for a project starts as soon as its
workspace dependencies finish. These commands follow the workspace package
graph, not the script relationships under `tasks`, and no longer wait for an
unrelated topological group to finish first.
