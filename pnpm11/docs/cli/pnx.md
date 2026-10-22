---
id: pnx
title: pnx
---

Aliases: `pnpm dlx`, `pnpx`

Fetches a package from the registry without installing it as a dependency, hotloads it, and runs whatever default command binary it exposes.

For example, to use `create-vue` anywhere to bootstrap a Vue project without
needing to install it under another project, you can run:

```sh
pnx create-vue my-app
```

This will fetch `create-vue` from the registry and run it with the given arguments.

You may also specify which exact version of the package you'd like to use:

```sh
pnx create-vue@next my-app
```

The `catalog:` protocol is also supported, allowing you to use versions defined in your workspace catalogs:

```sh
pnx shx@catalog:
```

## Options

### --package &lt;name\>

The package to install before running the command.

Example:

```sh
pnx --package=@pnpm/meta-updater meta-updater --help
pnx --package=@pnpm/meta-updater@0 meta-updater --help
```

Multiple packages can be provided for installation:

```sh
pnx --package=yo --package=generator-webapp yo webapp --skip-install
```

### --allow-build

Added in: v10.2.0

A list of package names that are allowed to run postinstall scripts during installation.

Example:

```sh
pnx --allow-build=esbuild my-bundler bundle
```

The actual packages executed by `dlx` are allowed to run postinstall scripts by default. So if in the above example `my-bundler` has to be built before execution, it will be built.

### --shell-mode, -c

Runs the command inside of a shell. Uses `/bin/sh` on UNIX and `\cmd.exe` on Windows.

Example:

```sh
pnx --package cowsay --package lolcatjs -c 'echo "hi pnpm" | cowsay | lolcatjs'
```

### --silent, -s

Only the output of the executed command is printed.

## Security and trust policies

Since v11.0.0, `pnx` (and its `pnpm dlx` / `pnpx` aliases) honors the project-level security and trust policy settings when resolving and fetching the requested package:

* [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage), [`minimumReleaseAgeExclude`](../settings/dependency-resolution.md#minimumreleaseageexclude), [`minimumReleaseAgeStrict`](../settings/dependency-resolution.md#minimumreleaseagestrict)
* [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy), [`trustPolicyExclude`](../settings/dependency-resolution.md#trustpolicyexclude), [`trustPolicyIgnoreAfter`](../settings/dependency-resolution.md#trustpolicyignoreafter)

This means `pnx` will refuse to execute freshly published or insufficiently trusted packages the same way a regular `pnpm install` would.
