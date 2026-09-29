---
id: exec
title: pnpm exec
---

Execute a shell command in scope of a project.

`node_modules/.bin` is added to the `PATH`, so `pnpm exec` allows executing commands of dependencies.

## Examples

If you have Jest as a dependency of your project, there is no need to install Jest globally, just run it with `pnpm exec`:

```
pnpm exec jest
```

The `exec` part is actually optional when the command is not in conflict with a builtin pnpm command, so you may also just run:

```
pnpm jest
```

## Options

Any options for the `exec` command should be listed before the `exec` keyword.
Options listed after the `exec` keyword are passed to the executed command.

Good. pnpm will run recursively:

```
pnpm -r exec jest
```

Bad, pnpm will not run recursively but `jest` will be executed with the `-r` option:

```
pnpm exec jest -r
```

### --recursive, -r

Execute the shell command in every project of the workspace.

The name of the current package is available through the environment variable
`PNPM_PACKAGE_NAME`.

#### Examples

Prune `node_modules` installations for all packages:

```
pnpm -r exec rm -rf node_modules
```

View package information for all packages. This should be used with the `--shell-mode` (or `-c`) option for the environment variable to work.

```
pnpm -rc exec pnpm view \$PNPM_PACKAGE_NAME
```

### --resume-from &lt;package_name\>

Resume execution from a particular project. This can be useful if you are working with a large workspace and you want to restart a build at a particular project without running through all of the projects that precede it in the build order.

### --parallel

Completely disregard concurrency and topological sorting, running a given script
immediately in all matching packages with prefixed streaming output. This is the
preferred flag for long-running processes over many packages, for instance, a
lengthy build process.

### --shell-mode, -c

Runs the command inside of a shell. Uses `/bin/sh` on UNIX and `\cmd.exe` on Windows.

### --report-summary

[Read about this option in the run command docs](./run.md#--report-summary)

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
