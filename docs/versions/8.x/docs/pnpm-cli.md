---
id: pnpm-cli
title: pnpm CLI
---

## Differences vs npm

Unlike npm, pnpm validates all options. For example, `pnpm install --target_arch x64` will
fail as `--target_arch` is not a valid option for `pnpm install`.

However, some dependencies may use the `npm_config_` environment variable, which
is populated from the CLI options. In this case, you have the following options:

1. explicitly set the env variable: `npm_config_target_arch=x64 pnpm install`
1. force the unknown option with `--config.`: `pnpm install --config.target_arch=x64`

## Options

### -C &lt;path\>, --dir &lt;path\>

Run as if pnpm was started in `<path>` instead of the current working directory.

### -w, --workspace-root

Run as if pnpm was started in the root of the [workspace](./workspaces.md)
instead of the current working directory.

## Commands

For more information, see the documentation for individual CLI commands. Here is
a list of handy npm equivalents to get you started:

| npm command     | pnpm equivalent    |
|-----------------|--------------------|
| `npm install`   | [`pnpm install`]   |
| `npm i <pkg>`   | [`pnpm add <pkg>`] |
| `npm run <cmd>` | [`pnpm <cmd>`]     |

When an unknown command is used, pnpm will search for a script with the given name,
so `pnpm run lint` is the same as `pnpm lint`. If there is no script with the specified name,
then pnpm will execute the command as a shell script, so you can do things like `pnpm eslint` (see [pnpm exec]).

[`pnpm install`]: ./cli/install.md
[`pnpm add <pkg>`]: ./cli/add.md
[`pnpm <cmd>`]: ./cli/run.md
[pnpm exec]: ./cli/exec.md

## Environment variables

Some environment variables that are not pnpm related might change the behaviour of pnpm:

* [`CI`](./cli/install.md#frozen-lockfile)

These environment variables may influence what directories pnpm will use for storing global information:

* `XDG_CACHE_HOME`
* `XDG_CONFIG_HOME`
* `XDG_DATA_HOME`
* `XDG_STATE_HOME`

You can search the docs to find the settings that leverage these environment variables.
