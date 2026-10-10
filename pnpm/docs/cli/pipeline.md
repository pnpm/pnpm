---
id: pipeline
title: "pnpm pipeline"
---

Added in: v12.4.0 (pnpm v12 only)

:::warning

`pnpm pipeline` is experimental. Its configuration and output may change.

:::

Runs a named set of workspace tasks the way a CI job would: a frozen install first, then the affected projects' task graph, with cached results restored instead of re-run.

```sh
pnpm pipeline [name]
```

The pipeline named `default` runs when no name is given.

## Declaring a pipeline

A pipeline is a set of task names, listed under `pipelines` in `pnpm-workspace.yaml`. It is a set, not a sequence: the order in which its tasks run comes from [`tasks.dependsOn`](../workspace-task-orchestration.md#configure-task-dependencies).

```yaml title="pnpm-workspace.yaml"
tasks:
  build:
    dependsOn: ['^build']
    outputs: ['dist/**']
  test:
    dependsOn: ['build']
    outputs: []
  lint:
    outputs: []

pipelines:
  default: [build, test, lint]
  release: [build]
```

```sh
pnpm pipeline          # build, test and lint
pnpm pipeline release  # build only
```

## What a run does

1. Installs with a frozen lockfile.
2. Selects the projects affected since the base revision, plus the projects that depend on them.
3. Builds the task graph over that selection and runs it.
4. Restores each cacheable task whose key it has seen before, and runs the rest.
5. Keeps going after a task fails, so one run reports every failure rather than the first.

The workspace root is left out of the selection unless [`includeWorkspaceRoot`](../workspaces.md#includeworkspaceroot) is `true`.

Use `--dry-run` to see the graph without installing configuration dependencies, running workspace hooks, or executing anything. Add `--json` for the nodes and edges.

## Caching a task

A task becomes cacheable by declaring `outputs`. Declaring `outputs: []` is the positive statement that a task produces no files, which is what makes a linter or a test run cacheable. A task with no `outputs` key runs every time.

```yaml title="pnpm-workspace.yaml"
tasks:
  build:
    dependsOn: ['^build']
    outputs: ['dist/**']
    inputs: ['src/**', 'tsconfig.json']
    env: ['NODE_ENV']
```

| Key | Meaning |
| --- | --- |
| `outputs` | Globs, relative to the project directory, naming the files the task produces. Their presence makes the task cacheable. |
| `inputs` | Globs narrowing what goes into the cache key. Without it, every tracked file of the project counts. An entry prefixed with `+` adds to that default instead of replacing it. |
| `env` | Environment variable names whose values take part in the key. The values are hashed, never stored. |
| `cache` | `false` opts a task with declared `outputs` back out of the cache. |

Besides the declared inputs, a task's key covers the script text, the keys of the tasks it depends on, the lockfile, and the runtime. A cache hit restores the output files and replays the recorded log, so the run reads the same either way.

`--no-cache` runs everything and neither reads nor writes cache entries.

## Reusing Cargo build state

A [Cargo](../cargo.md) task can keep its local build state between runs, and between git worktrees of the same repository:

```yaml title="pnpm-workspace.yaml"
tasks:
  build:
    cargoTargetDir: target
```

pnpm points Cargo's target and build directories at that path and always executes the task, restoring an immutable snapshot of the previous build state rather than the task's own outputs. The directory must be relative to the project and ignored by git. Snapshots are stored apart from installed packages, so evicting one can never break an installation.

## Sharing the cache between machines

Added in: v12.12.0

A cacheable task's result can also be shared through the server [`remoteCache`](../settings/build.md#remotecache) names: a server that speaks the Turborepo Remote Cache API, such as Vercel Remote Cache, or a [pnpr](/pnpr) server. A CI run that publishes its results lets a later run on another machine restore them instead of running the tasks.

Results are signed artifacts, the same kind the [remote side-effects cache](../settings/build.md#sideeffectscacheremote) uses. A machine restores a result only when it is signed by one of its `trustedKeys`, so only the machines that hold a private key can publish results others will use.

```yaml title="pnpm-workspace.yaml"
remoteCache:
  org: acme
```

```yaml title="~/.config/pnpm/config.yaml"
remoteCache:
  url: https://vercel.com/api
  team: team_abc123
  trustedKeys:
    ci-2026: <base64 public key>
```

On the machine that publishes, usually CI, add the private key and turn publishing on. A published result includes the task's output files and its captured terminal output, so publish only from machines whose logs every reader may see:

```sh
pnpm config set //vercel.com/api/:_authToken "$VERCEL_TOKEN"
export PNPM_REMOTE_CACHE_URL=https://vercel.com/api
export PNPM_REMOTE_CACHE_TEAM=team_abc123
export PNPM_REMOTE_CACHE_TRUSTED_KEYS="{\"ci-2026\":\"$PUBLIC_KEY\"}"
export PNPM_REMOTE_CACHE_PRIVATE_KEY="$PRIVATE_KEY"
export PNPM_REMOTE_CACHE_KEY_ID=ci-2026
export PNPM_REMOTE_CACHE_BUILDER_ID=ci
export PNPM_REMOTE_CACHE_PUBLISH=true
pnpm pipeline
```

See [Shared side-effects cache](/pnpr/shared-side-effects-cache#publishing-from-a-builder) for how to generate a key pair.

A remote result that is missing, untrusted, or fails verification is a miss, and the task runs. A server that cannot be reached never fails the run. A task's outputs are published when they are at most 10,000 files, each at most 4 GiB and all together at most 16 GiB. [Cargo build state](#reusing-cargo-build-state) stays on the local machine.

## Reporting a run

Every run writes a summary and an event stream under pnpm's pipeline data directory. `--report` submits them to the [pnpr](/pnpr/pipeline-runs) server named by [`pnprServer`](/pnpr/install-acceleration), under the organization [`remoteCache.org`](../settings/build.md#remotecache) names; `--report-to <url>` submits them elsewhere, which is the better spelling when the server that stores runs is not the one that accelerates installs. A failed run is reported before the command exits non-zero.

## Watching a repository

```sh
pnpm pipeline --repo https://github.com/acme/app.git --watch
```

The watch agent polls the repository and runs the pipeline for every new revision of the followed branch. It is a proof of concept, not a hosted CI service.

## Options

### --dry-run

Print the task graph and exit, without installing or running anything.

### --json

With `--dry-run`, print the tasks and their resolved dependency edges as JSON.

### --full

Run over every workspace project instead of the affected-since-base selection.

### --base &lt;ref&gt;

The git ref the affected selection diffs against, through its merge base with `HEAD`. Overrides the `pipelineBase` setting, which itself defaults to `origin/main`.

### --no-cache

Run every task, reading and writing neither task results nor Cargo snapshots.

### --report

Publish the run's summary and event stream to the configured pnpr server once the run settles.

### --report-to &lt;url&gt;

Publish the run to this pnpr server instead of the one in `pnprServer`.

### --repo &lt;repo&gt;

The repository the watch agent polls: a URL or a local path, anything git accepts as a remote.

### --watch

Watch `--repo` and run the pipeline for every new revision instead of running once in the current directory.

### --branch &lt;name&gt;

The branch the watch agent follows. Defaults to `main`.

### --interval &lt;seconds&gt;

Seconds between polls of the watched repository. Defaults to `30`.

### --once

With `--watch`, poll once, build if there is a new revision, and exit.

## Settings

### pipelines

* Default: **undefined**
* Type: **Record&lt;string, string[]&gt;**

Named sets of task names, keyed by pipeline name.

### pipelineBase

* Default: **origin/main**
* Type: **String**

The git ref the affected selection resolves its merge base against.
