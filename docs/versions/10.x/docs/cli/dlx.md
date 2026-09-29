---
id: dlx
title: "pnpm dlx"
---

Aliases: `pnpx` is an alias for `pnpm dlx`

Fetches a package from the registry without installing it as a dependency, hotloads it, and runs whatever default command binary it exposes.

For example, to use `create-vue` anywhere to bootstrap a Vue project without
needing to install it under another project, you can run:

```
pnpm dlx create-vue my-app
```

This will fetch `create-vue` from the registry and run it with the given arguments.

You may also specify which exact version of the package you'd like to use:

```
pnpm dlx create-vue@next my-app
```

The `catalog:` protocol is also supported, allowing you to use versions defined in your workspace catalogs:

```
pnpm dlx shx@catalog:
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

### --allow-build

Added in: v10.2.0

A list of package names that are allowed to run postinstall scripts during installation.

Example:

```
pnpm --allow-build=esbuild my-bundler bundle
```

The actual packages executed by `dlx` are allowed to run postinstall scripts by default. So if in the above example `my-bundler` has to be built before execution, it will be built.

### --shell-mode, -c

Runs the command inside of a shell. Uses `/bin/sh` on UNIX and `\cmd.exe` on Windows.

Example:

```
pnpm --package cowsay --package lolcatjs -c dlx 'echo "hi pnpm" | cowsay | lolcatjs'
```

### --silent, -s

Only the output of the executed command is printed.
