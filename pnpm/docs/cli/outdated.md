---
id: outdated
title: pnpm outdated
---

Checks for outdated packages. The check can be limited to a subset of the
installed packages by providing arguments. Patterns support `*` wildcards, and a
leading `!` excludes matching packages.

`pnpm outdated` exits with code 1 when any dependency is outdated.

Examples:
```sh
pnpm outdated
pnpm outdated "*gulp-*" @babel/core
```

## Options

### --recursive, -r

Check for outdated dependencies in every package found in subdirectories, or in
every workspace package when executed inside a workspace.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

### --global, -g

List outdated global packages.

### --long

Print details.

### --format &lt;format\>

* Default: **table**
* Type: **table**, **list**, **json**

Prints the outdated dependencies in the given format.

### --json

Shorthand for `--format json`.

### --no-table

Shorthand for `--format list`.

### --compatible

Prints only versions that satisfy specifications in `package.json`.

### --dev, -D

Checks only `devDependencies`.

### --prod, -P, --production

Checks only `dependencies` and `optionalDependencies`.

### --no-optional

Doesn't check `optionalDependencies`.

### --optional

Checks `optionalDependencies`.

### --sort-by

Specifies the order in which the output results are sorted. Currently only the value `name` is accepted. By default, results are sorted by the size of the version change, then by name.

### --include-github-actions

Also check the GitHub Actions referenced by the repository's workflow files for updates. You can enable this by default by setting [`update.githubActions`](../settings/dependency-resolution.md#updategithubactions) to `true` in `pnpm-workspace.yaml`. See [Updating GitHub Actions](./update.md#updating-github-actions).
