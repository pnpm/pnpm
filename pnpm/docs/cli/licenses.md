---
id: licenses
title: pnpm licenses
---

## Commands

### list

Aliases: `ls`

List licenses for installed packages.

Since v12.7.0, running `pnpm licenses list` inside a workspace project lists only
that project's dependencies. Use [`--recursive`](#--recursive--r) or [`--filter`](#--filter-package_selector)
to include other workspace projects.

Since v11.20.0, a package resolved from a [named registry](../settings/dependency-resolution.md#namedregistries) is reported separately from a package of the same name and version that came from another registry. The registry alias is shown next to the package name in the table output and is exposed as the `registryName` field with `--json`.

## Options

### --recursive, -r

List the licenses of the dependencies of every workspace project.

### --dev, -D

Check only "devDependencies".

### --json

Show information in JSON format.

### --long

Show more details (such as a link to the repo) are not displayed. To display the details, pass this option.

### --no-optional

Don't check packages from `optionalDependencies`.

### --prod, -P

Check only `dependencies` and `optionalDependencies`.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
