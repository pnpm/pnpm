---
id: dlx
title: "pnpm dlx"
---

Fetches a package from the registry without installing it as a dependency, hotloads it, and runs whatever default command binary it exposes.

For example, to use `create-react-app` anywhere to bootstrap a react app without
needing to install it under another project, you can run:

```
pnpm dlx create-react-app ./my-app
```

This will fetch `create-react-app` from the registry and run it with the given arguments.

You may also specify which exact version of the package you'd like to use:

```
pnpm dlx create-react-app@next ./my-app
```

## Options

### --package &lt;name\>

The package to install before running the command.

Example:

```
pnpm --package=@pnpm/meta-updater dlx meta-updater --help
pnpm --package=@pnpm/meta-updater@0 dlx meta-updater --help
```

Multiple packages can be provided for installation:

```
pnpm --package=yo --package=generator-webapp dlx yo webapp --skip-install
```

### --shell-mode, -c

Runs the command inside of a shell. Uses `/bin/sh` on UNIX and `\cmd.exe` on Windows.

Example:

```
pnpm --package cowsay --package lolcatjs -c dlx 'echo "hi pnpm" | cowsay | lolcatjs'
```

### --silent, -s

Only the output of the executed command is printed.
