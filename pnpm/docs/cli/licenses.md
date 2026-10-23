---
id: licenses
title: pnpm licenses
---

Aliases: `licences`

Checks the licenses of the installed packages.

```sh
pnpm licenses list
```

`pnpm licenses` without a subcommand fails with `ERR_PNPM_LICENCES_NO_SUBCOMMAND`.

## Commands

### list

Aliases: `ls`

List licenses for installed packages.

The command requires a `pnpm-lock.yaml` file. A workspace project not yet
recorded in the lockfile has no licenses to list. When no licenses match, text
output says `No licenses in packages found` and `--json` outputs `{}`.

Since v12.7.0, running `pnpm licenses list` inside a workspace project lists only
that project's dependencies. Use [`--recursive`](#--recursive--r) or [`--filter`](#--filter-package_selector)
to include other workspace projects.

A package resolved from a [named registry](../settings/dependency-resolution.md#namedregistries) is reported separately from a package of the same name and version that came from another registry. The registry alias is shown next to the package name in the table output and is exposed as the `registryName` field with `--json`.

## Options

### --recursive, -r

List the licenses of the dependencies of every workspace project.

### --dev, -D

Check only "devDependencies".

### --json

Show information in JSON format.

### --long

Show the author, description, and homepage of each package.

### --no-optional

Don't check packages from `optionalDependencies`.

### --optional, -O

Check only `optionalDependencies`.

### --prod, -P, --production

Check only `dependencies` and `optionalDependencies`.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
