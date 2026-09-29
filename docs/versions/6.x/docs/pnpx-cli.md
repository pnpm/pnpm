---
id: pnpx-cli
title: pnpx CLI
---

:::warning

This command is deprecated! Use [`pnpm exec`] and [`pnpm dlx`] instead.

:::

## For new users

`pnpx` (PNPm eXecute) is a command line tool that fetches a package from the
registry without installing it as a dependency, hotloads it, and runs whatever
default command binary it exposes.

For example, to use `create-react-app` anywhere to bootstrap a react app without
needing to install it under another project, you can run:

```sh
pnpx create-react-app my-project
```

This will fetch `create-react-app` from the registry and run it with the given
arguments. For more information, you may want to look at [npx] from npm, as it
offers the same interface, except it uses `npm` instead of `pnpm` under the hood.

If you just want to run a binary of a project's dependency, see [`pnpm exec`].

## For npm users

npm has a great package runner called [npx]. pnpm offers the same tool via the
`pnpx` command. The only difference is that `pnpx` uses `pnpm` for installing
packages.

[npx]: https://www.npmjs.com/package/npx
[`pnpm exec`]: ./cli/exec.md
[`pnpm dlx`]: ./cli/dlx.md
