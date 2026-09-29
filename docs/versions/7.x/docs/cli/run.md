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

Added in: v7.27.0

You may run multiple scripts at the same time by using a regex instead of the script name.

```sh
pnpm run "/<regex>/"
```

Run all scripts that start with `watch:`:

```sh
pnpm run "/^watch:.*/"
```

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

## Differences with `npm run`

By default, pnpm doesn't run arbitrary `pre` and `post` hooks for user-defined
scripts (such as `prestart`). This behavior, inherited from npm, caused scripts
to be implicit rather than explicit, obfuscating the execution flow. It also led
to surprising executions with `pnpm serve` also running `pnpm preserve`.

If for some reason you need the pre/post scripts behavior of npm, use the
`enable-pre-post-scripts` option.

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

### script-shell

* Default: **null**
* Type: **path**

The shell to use for scripts run with the `pnpm run` command.

For instance, to force usage of Git Bash on Windows:

```
pnpm config set script-shell "C:\\Program Files\\git\\bin\\bash.exe"
```

### shell-emulator

* Default: **false**
* Type: **Boolean**

When `true`, pnpm will use a JavaScript implementation of a [bash-like shell] to
execute scripts.

This option simplifies cross-platform scripting. For instance, by default, the
next script will fail on non-POSIX-compliant systems:

```json
"scripts": {
  "test": "NODE_ENV=test node test.js"
}
```

But if the `shell-emulator` setting is set to `true`, it will work on all
platforms.

[bash-like shell]: https://www.npmjs.com/package/@yarnpkg/shell

### --recursive, -r

This runs an arbitrary command from each package's "scripts" object.
If a package doesn't have the command, it is skipped.
If none of the packages have the command, the command fails.

### --if-present

You can use the `--if-present` flag to avoid exiting with a non-zero exit code
when the script is undefined. This lets you run potentially undefined scripts
without breaking the execution chain.

### --parallel

Completely disregard concurrency and topological sorting, running a given script
immediately in all matching packages with prefixed streaming output. This is the
preferred flag for long-running processes over many packages, for instance, a
lengthy build process.

### --stream

Stream output from child processes immediately, prefixed with the originating
package directory. This allows output from different packages to be interleaved.

### --aggregate-output

Aggregate output from child processes that are run in parallel, and only print output when the child process is finished. It makes reading large logs after running `pnpm -r <command>` with `--parallel` or with `--workspace-concurrency=<number>` much easier (especially on CI). Only `--reporter=append-only` is supported.

### enable-pre-post-scripts

* Default: **false**
* Type: **Boolean**

When `true`, pnpm will run any pre/post scripts automatically. So running `pnpm foo`
will be like running `pnpm prefoo && pnpm foo && pnpm postfoo`.

### --resume-from &lt;package_name\>

Added in: v7.22.0

Resume execution from a particular project. This can be useful if you are working with a large workspace and you want to restart a build at a particular project without running through all of the projects that precede it in the build order.

### --report-summary

Added in: v7.28.0

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

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
