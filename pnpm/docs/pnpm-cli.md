---
id: pnpm-cli
title: pnpm CLI
---

## Short aliases

`pn` is available as a short alias for `pnpm`, and [`pnx`](./cli/pnx.md) as a short alias for `pnpm dlx`. You can use them anywhere you'd use `pnpm` or `pnpx`:

```sh
pn install
pn add express
pn build
pn test
pnx create-vue my-app
```

## Differences vs npm

Unlike npm, pnpm validates all options. For example, `pnpm install --target_arch x64` will
fail as `--target_arch` is not a valid option for `pnpm install`.

Some dependencies read an `npm_config_` environment variable during their build.
pnpm does not set these variables from its options, so set the variable yourself:
`npm_config_target_arch=x64 pnpm install`.

## Boolean flags

A boolean flag can be written on its own, negated with a `no-` prefix, or given an explicit value:

```sh
pnpm install --prod            # devDependencies are skipped
pnpm install --no-prod         # devDependencies are installed
pnpm install --prod=false      # the same, since v12.4.0
pnpm install --prod=true       # the same as --prod
```

The `--config.` escape hatch takes a value the same way: `--config.trust-lockfile=false`.

## Which project a command acts on

Run from a subdirectory of a project, a command acts on the nearest ancestor
directory that has a manifest, so `pnpm bin` from `packages/app/src` reports
paths under `packages/app`. Two commands stay where you are: `pnpm init`
creates its manifest in the current directory, and `pnpm exec` runs the command
there.

## Options

### -C &lt;path\>, --dir &lt;path\>

Run as if pnpm was started in `<path>` instead of the current working directory.

### -w, --workspace-root

Run as if pnpm was started in the root of the [workspace](./workspaces.md)
instead of the current working directory.

### --ignore-workspace

Run as if the current project were standalone. pnpm does not discover a
`pnpm-workspace.yaml` in a parent directory, so the workspace's settings and
other projects do not apply to the command.

For example, this installs only the project in the current directory, even if
it is located inside a workspace:

```sh
pnpm install --ignore-workspace
```

### --workspace-packages &lt;glob\>

Select the workspace's projects with a glob pattern, overriding the
[`packages`](./settings.md#packages) field of `pnpm-workspace.yaml`. Repeat
the flag to add more patterns.

### -y, --yes

Automatically answer yes to prompts.

### --env-file &lt;path\>

Added in: v12.10.0

Load environment variables from a dotenv-style file before running. Repeat
the flag to layer more files:

```sh
pnpm --env-file .env run build
pnpm --env-file .env --env-file .env.local install
```

Later files override variables defined in earlier files, and variables
already in the environment are never overridden. Relative paths resolve
against the working directory where `pnpm` was invoked. The loaded variables
are visible to config resolution (`PNPM_CONFIG_<KEY>` overrides and `${VAR}`
tokens in user-level [`.npmrc`](./npmrc.md) files, so registry tokens can
live in the env file instead of the shell), to lifecycle scripts, and to any
pnpm the command dispatches to.

Values support `$VAR` / `${VAR}` substitution from the environment and
earlier entries, and `export ` prefixes, following dotenv conventions;
single-quote a value to keep a literal `$`. A leading UTF-8 byte-order mark
is tolerated. A missing or malformed file
fails the command.

The flag is pnpm's own: a shim invocation forwards its arguments to the
shim target untouched and never loads env files.

## Commands

For more information, see the documentation for individual CLI commands. Here is
a list of handy npm equivalents to get you started:

| npm command     | pnpm equivalent    |
|-----------------|--------------------|
| `npm install`   | [`pnpm install`]     |
| `npm i <pkg>`   | [`pnpm add <pkg>`]   |
| `npm run <cmd>` | [`pnpm <cmd>`]       |
| `npx <pkg>`     | [`pnx <pkg>`]      |

When an unknown command is used, pnpm will search for a script with the given name,
so `pnpm run lint` is the same as `pnpm lint`. If there is no script with the specified name,
then pnpm will execute the command as a shell script, so you can do things like `pnpm eslint` (see [`pnpm exec`]).

[`pnpm install`]: ./cli/install.md
[`pnpm add <pkg>`]: ./cli/add.md
[`pnpm <cmd>`]: ./cli/run.md
[`pnpm exec`]: ./cli/exec.md
[`pnx <pkg>`]: ./cli/pnx.md

## Environment variables

Some environment variables that are not pnpm related might change the behaviour of pnpm:

* [`CI`](./cli/install.md#--frozen-lockfile)
* `NO_COLOR` turns off colored output.

These environment variables configure pnpm itself:

* `pnpm_config_<setting>` sets a setting. See [Environment variables](./configuring.md#environment-variables).
* `PNPM_HOME` sets the pnpm home directory, which holds global packages, their bins, and the default store.
* `PNPM_SHIM_BYPASS=1` makes a global shim run its global target and ignore the project's pin. See [project-aware global bins](./global-packages.md#project-aware-global-bins).

These environment variables may influence what directories pnpm will use for storing global information:

* `XDG_CACHE_HOME`
* `XDG_CONFIG_HOME`
* `XDG_DATA_HOME`
* `XDG_STATE_HOME`

You can search the docs to find the settings that use these environment variables.
