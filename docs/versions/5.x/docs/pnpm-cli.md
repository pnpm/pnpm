---
id: pnpm-cli
title: pnpm CLI
---

## Differences vs npm

Unlike npm, pnpm validates all options. For example, `pnpm install --foo` will
fail as `--foo` is not a valid option for `pnpm install`.

However, some dependencies may use the `npm_config_` environment variable, which
is populated from the CLI options. In this case, you have the following options:

1. explicitly set the env variable: `npm_config_foo=true pnpm install`
1. force the unknown option with `--config.`: `pnpm install --config.foo=true`

## Options

### -C &lt;path\>, --dir &lt;path\>

Run as if pnpm was started in `<path>` instead of the current working directory.

### -w, --workspace-root

Added in: v5.6.0

Run as if pnpm was started in the root of the workspace instead of the current
working directory.

## Commands

For more information, see the documentation for individual CLI commands. Here is
a list of handy npm equivalents to get you started:

| npm command     | pnpm equivalent    |
|-----------------|--------------------|
| `npm install`   | [`pnpm install`]   |
| `npm i <pkg>`   | [`pnpm add <pkg>`] |
| `npm run <cmd>` | [`pnpm <cmd>`]     |

[`pnpm install`]: ./cli/install.md
[`pnpm add <pkg>`]: ./cli/add.md
[`pnpm <cmd>`]: ./cli/exec.md
