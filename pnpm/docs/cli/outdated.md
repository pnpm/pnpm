---
id: outdated
title: pnpm outdated
---

Checks for outdated packages, runtimes (`node`, `deno`, `bun` declared in
`devEngines.runtime`, `engines.runtime`, or through the `runtime:` / `tool:`
protocol), package managers (`pnpm`, `npm`, `yarn`, `bun` declared in
`devEngines.packageManager` or `packageManager`), Cargo crates (`Cargo.toml`
when [`cargo.enabled`](../cargo.md#cargoenabled) is `true`), and Python packages
(`pyproject.toml` when [`python.enabled`](../python.md#pythonenabled) is
`true`). The check can be limited to a subset of the declared dependencies,
runtimes, or package managers by providing arguments, including `crate:<name>`
for Cargo crates and `pypi:<name>` for Python packages. Patterns support `*`
wildcards, and a leading `!` excludes matching names.

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

### --notes

Print the release notes between the current and the latest version below each outdated package. Notes are read from registries declared with [`serverType: pnpr`](../registries.md#servertype). The notes of a release that published several packages are printed once, above the notes of the versions it published. Between two stable versions, the notes of prereleases are left out. With `--json`, each entry gets a `releaseNotes` field.

### --include-github-actions

Also check the GitHub Actions referenced by the repository's workflow files for updates. You can enable this by default by setting [`update.githubActions`](../settings/dependency-resolution.md#updategithubactions) to `true` in `pnpm-workspace.yaml`. See [Updating GitHub Actions](./update.md#updating-github-actions).
