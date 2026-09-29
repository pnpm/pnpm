---
id: dlx
title: "pnpm dlx"
---

Added in: v6.13.0

Fetches a package from the registry without installing it as a dependency, hotloads it, and runs whatever default command binary it exposes.

For example, to use `create-react-app` anywhere to bootstrap a react app without
needing to install it under another project, you can run:

```
pnpm dlx create-react-app ./my-app
```

This will fetch `create-react-app` from the registry and run it with the given arguments.

Since v6.23.1 you may also specify which exact version of the package you'd like to use:

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

As of v6.14.5, multiple packages can be provided for installation:

```
pnpm --package=yo --package=generator-webapp dlx yo webapp --skip-install
```

### --silent, -s

Added in: v6.17.0

Only the output of the executed command is printed.
