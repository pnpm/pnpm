---
id: run
title: pnpm run
---

Aliases: `run-script`

Runs a script defined in the package's manifest file.

## Examples

Let's say you have a `watch` script configured in your `package.json`, like so:

```json
"scripts": {
    "watch": "webpack --watch"
}
```

You can now run that script by using `pnpm run watch`! Simple, right?
Another thing to note for those that like to save keystrokes and time is that
all scripts get aliased in as pnpm commands, so ultimately `pnpm watch` is just
shorthand for `pnpm run watch` (ONLY for scripts that do not share the same name
as already existing pnpm commands).

## Running multiple scripts

You may run multiple scripts at the same time by using a regex instead of the script name.

```sh
pnpm run "/<regex>/"
```

Run all scripts that start with `watch:`:

```sh
pnpm run "/^watch:.*/"
```

The selector must be written as a regular expression literal — that is, wrapped in slashes — and quoted, so the shell does not mangle it. A plain string is always treated as a literal script name, and a script whose name matches the argument exactly takes precedence over regex matching.

Matching is not anchored, so `"/build:.*/"` also matches `prebuild:web`. Anchor the pattern with `^` and `$` when you need an exact prefix.

Matched scripts run in lexicographical order, so the selection is deterministic regardless of the order the scripts appear in `package.json`. To run them strictly one at a time, add [`--sequential`](#--sequential--s).

Regular expression flags are not supported: `pnpm run "/^build:.*/i"` fails with `ERR_PNPM_UNSUPPORTED_SCRIPT_COMMAND_FORMAT`.

## Details

In addition to the shell’s pre-existing `PATH`, `pnpm run` includes
`node_modules/.bin` in the `PATH` provided to `scripts`. This means that so
long as you have a package installed, you can use it in a script like a regular
command. For example, if you have `eslint` installed, you can write up a script
like so:

```json
"lint": "eslint src --fix"
```

And even though `eslint` is not installed globally in your shell, it will run.

For workspaces, `<workspace root>/node_modules/.bin` is also added
to the `PATH`, so if a tool is installed in the workspace root, it may be called
in any workspace package's `scripts`.

## Environment

There are some environment variables that pnpm automatically creates for the executed scripts.
These environment variables may be used to get contextual information about the running process.

These are the environment variables created by pnpm:

* **npm_command** - contains the name of the executed command. If the executed command is `pnpm run`, then the value of this variable will be "run-script".

## Options

Any options for the `run` command should be listed before the script's name.
Options listed after the script's name are passed to the executed script.

All these will run pnpm CLI with the `--silent` option:

```sh
pnpm run --silent watch
pnpm --silent run watch
pnpm --silent watch
```

Any arguments after the command's name are added to the executed script.
So if `watch` runs `webpack --watch`, then this command:

```sh
pnpm run watch --no-color
```

will run:

```sh
webpack --watch --no-color
```

### --recursive, -r

This runs an arbitrary command from each package's "scripts" object.
If a package doesn't have the command, it is skipped.
If none of the packages have the command, the command fails.

By default, recursive runs use a dependency-aware task graph. Configure
relationships between scripts with the
[`tasks` setting](../workspace-task-orchestration.md). `--no-sort`, and
`--parallel` which implies it, remove that ordering and ignore the `tasks`
declarations along with it.

### --if-present

You can use the `--if-present` flag to avoid exiting with a non-zero exit code
when the script is undefined. This lets you run potentially undefined scripts
without breaking the execution chain.

### --no-bail

Continue running the remaining matched scripts even if one of them fails. The command still exits with a non-zero exit code if any script failed.

### --parallel

Completely disregard concurrency and topological sorting, running a given script
immediately in all matching packages with prefixed streaming output. This is the
preferred flag for long-running processes over many packages, for instance, a
lengthy build process.

### --sequential, -s

Added in: v11.14.0

Run the selected scripts one by one. This forces [`--workspace-concurrency`](./recursive.md#--workspace-concurrency) to `1`, so scripts matched by a [regex selector](#running-multiple-scripts) never overlap — neither across workspace packages nor within a single package.

```sh
pnpm run --sequential "/^build:.*/"
```

In a recursive run this serializes scripts across workspace projects as well as within each one. `--sequential` takes precedence over `--parallel`: concurrency is pinned to `1` whenever it is set, regardless of the order the two flags appear in.

:::note

For `pnpm run`, `-s` is the shorthand for `--sequential`. Everywhere else in the CLI, `-s` remains the shorthand for `--reporter=silent`. The long form `--silent` is unaffected in all commands.

:::

### --stream

Stream output from child processes immediately, prefixed with the originating
package directory. This allows output from different packages to be interleaved.

### --aggregate-output

Aggregate output from child processes that are run in parallel, and only print output when the child process is finished. It makes reading large logs after running `pnpm -r <command>` with `--parallel` or with `--workspace-concurrency=<number>` much easier (especially on CI). Only `--reporter=append-only` is supported.

### --resume-from &lt;package_name\>

Resume at the named package's requested task. pnpm skips the tasks a matching
record of the previous run says already passed; without such a record it omits
the task's transitive dependencies instead, treating them as completed. Either
way it still runs the task itself, its dependents, and unrelated tasks in the
selected graph. See
[Workspace task orchestration](../workspace-task-orchestration.md#--resume-from-package_name).

### --dry-run

Resolve a recursive run's task graph without running scripts. The plain output
is one stable topological ordering, not a prediction of the order in which
independent tasks will be dispatched.

```sh
pnpm -r run --dry-run build
```

`--dry-run` is only supported for recursive runs.

### --json

With `--dry-run`, print the resolved tasks and their dependency edges as JSON.
See [Inspect the task graph](../workspace-task-orchestration.md#inspect-the-task-graph)
for the schema and an example.

### --report-summary

Record the result of the scripts executions into a `pnpm-exec-summary.json` file.

An example of a `pnpm-exec-summary.json` file:

```json
{
  "executionStatus": {
    "/Users/zoltan/src/pnpm/pnpm/cli/command": {
      "status": "passed",
      "duration": 1861.143042
    },
    "/Users/zoltan/src/pnpm/pnpm/cli/common-cli-options-help": {
      "status": "passed",
      "duration": 1865.914958
    }
  }
```

Possible values of `status` are: 'passed', 'queued', 'running'.

### --reporter-hide-prefix

Hide workspace prefix from output from child processes that are run in parallel, and only print the raw output. This can be useful if you are running on CI and the output must be in a specific format without any prefixes (e.g. [GitHub Actions annotations](https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions#setting-an-error-message)). Only `--reporter=append-only` is supported.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

## pnpm-workspace.yaml settings

The [`tasks` setting](../workspace-task-orchestration.md) configures dependency
relationships and per-task concurrency limits for recursive runs.

import EnablePrePostScripts from '../settings/_enablePrePostScripts.mdx'

<EnablePrePostScripts />

import ScriptShell from '../settings/_scriptShell.mdx'

<ScriptShell />

import ShellEmulator from '../settings/_shellEmulator.mdx'

<ShellEmulator />
