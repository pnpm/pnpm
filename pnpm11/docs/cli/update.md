---
id: update
title: pnpm update
---

Aliases: `up`, `upgrade`

`pnpm update` updates packages to their latest version based on the specified
range.

When used without arguments, updates all dependencies.

## TL;DR

| Command              | Meaning                                                                  |
|----------------------|--------------------------------------------------------------------------|
|`pnpm up`             | Updates all dependencies, adhering to ranges specified in `package.json` |
|`pnpm up --latest`    | Updates all dependencies to their latest versions                        |
|`pnpm up foo@2`       | Updates `foo` to the latest version on v2                                |
|`pnpm up "@babel/*"` | Updates all dependencies under the `@babel` scope                        |

Since v11.27.1, a dependency whose specifier comes from a [`packageExtensions`](../settings/dependency-resolution.md#packageextensions) entry, a [`readPackage`](../pnpmfile.md#hooks) hook, or an [`override`](../settings/dependency-resolution.md#overrides) is not written to `package.json`. The lockfile keeps that specifier. `pnpm update` and `pnpm audit --fix=update` still move it inside the range.

An exact pin from one of those sources is left as it is. When that pin is a vulnerable version, `pnpm audit --fix=update` warns and points at `pnpm audit --fix`, which writes an override. An override is applied after `packageExtensions` and `readPackage`, so it replaces a specifier those supply.

Since v11.25.0, [`--patches`](#--patches) changes no version and no declared range, refreshing only
which registry artifact each locked version resolves to. That still rewrites
lockfile metadata — the `integrity`, the `revision`, and the package snapshot
around them — because artifact revisions may declare different dependencies. See
[Registry revisions](../registry-revisions.md).

## Updating to a specific version

Since v11.23.0, `pnpm update <name>@<version>` fails with `ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP` when the package is not a direct dependency of any selected project. There is nowhere to record the version in that case; pin a transitive dependency through [`overrides`](../settings/dependency-resolution.md#overrides) instead. Ranges and tags are unaffected.

## Selecting dependencies with patterns

You can use patterns to update specific dependencies.

Update all `babel` packages:

```sh
pnpm update "@babel/*"
```

Update all dependencies, except `webpack`:

```sh
pnpm update "\!webpack"
```

Patterns may also be combined, so the next command will update all `babel` packages, except `core`:

```sh
pnpm update "@babel/*" "\!@babel/core"
```

## Updating GitHub Actions

Added in: v11.16.0

[`pnpm outdated`](./outdated.md) can check the GitHub Actions referenced by the repository's workflow files for updates, and `pnpm update` can update them. This is opt-in for every command: pass [`--include-github-actions`](#--include-github-actions), or set [`update.githubActions`](../settings/dependency-resolution.md#updategithubactions) to `true` in `pnpm-workspace.yaml` to enable it by default.

Updated actions are pinned to exact commit hashes, with their release tags preserved in comments:

```yaml
- uses: actions/checkout@08c6903cd8c0fde910a37f88322edcfb5dd907a8 # v5.0.0
```

Local actions are followed rather than looked up: a `./`-relative reference, and
since v11.26.0 GitHub's self-repository spelling
(`uses: $/.github/actions/setup`), resolve inside the repository.

Checking for updates runs `git ls-remote` against every referenced repository. Actions whose refs cannot be read — for example, an action in a private repository — are skipped with a warning. If the actions are hosted on a different GitHub server (such as a GitHub Enterprise Server), set [`update.githubActionsServer`](../settings/dependency-resolution.md#updategithubactionsserver) (added in v11.17.0).

## Options

### --recursive, -r

Concurrently runs update in all subdirectories with a `package.json` (excluding
node_modules).

Usage examples:

```sh
pnpm --recursive update
# updates all packages up to 100 subdirectories in depth
pnpm --recursive update --depth 100
# update typescript to the latest version in every package
pnpm --recursive update typescript@latest
```

### --latest, -L

Update the dependencies to their latest stable version as determined by their `latest` tags (potentially upgrading the packages across major versions) as long as the version range specified in `package.json` is lower than the `latest` tag (i.e. it will not downgrade prereleases).

Since v11.27.1, `--latest` does not move a dependency past a specifier supplied by `packageExtensions`, a `readPackage` hook, or an override. The lockfile stays inside that specifier.

### --patches

Added in: v11.25.0

Refresh [registry revisions](../registry-revisions.md) without changing any
package version: for every locked registry package, pnpm resolves current
metadata for the same `name@version`, and adopts the artifact the registry
selects now if that changed. Versions and declared ranges are left alone, but the
lockfile entry is rewritten as a whole — `integrity`, `revision`, and the package
snapshot around them — since revisions may declare different dependencies.

Cannot be combined with package selectors, `--latest`, `--interactive`, or
`--global` (`ERR_PNPM_PATCHES_WITH_SELECTOR`). Each of those narrows or redirects
what an update targets — a subset of packages, the newest versions, an
interactive choice, the global installation — and `--patches` is defined as
covering every locked package of the current project at its current version.

### --pnpr-server &lt;url\>

Added in: v11.25.0

Offload the resolution of a `--patches` refresh to a [pnpr](/pnpr) server, the
way [`pnprServer`](/pnpr/install-acceleration) does for an install.

### --global, -g

Update global packages.

### --workspace

Tries to link all packages from the workspace. Versions are updated to match the
versions of packages inside the workspace.

If specific packages are updated, the command will fail if any of the updated
dependencies are not found inside the workspace. For instance, the following
command fails if `express` is not a workspace package:

```sh
pnpm up -r --workspace express
```

### --prod, -P

Only update packages in `dependencies` and `optionalDependencies`.

### --dev, -D

Only update packages in `devDependencies`.

### --no-optional

Don't update packages in `optionalDependencies`.

### --peer

Added in: v11.28.0

Also update the ranges in `peerDependencies`. Without this flag, `pnpm update` leaves `peerDependencies` unchanged.

```sh
pnpm update --peer react
```

### --interactive, -i

Show outdated dependencies and select which ones to update.

Since v11.21.0, combined with `--global`, each [isolated install group](../global-packages.md#isolated-installations) is presented as one selectable item: packages that share a global installation update together as a unit.

### --no-save

Don't update the ranges in `package.json`.

### --changeset

Added in: v11.16.0

After the update completes, write a [change intent](../versioning.md) — a changesets-compatible `.changeset/*.md` file — declaring a `patch` bump for every workspace package whose `dependencies` or `optionalDependencies` were changed by the update, and a `major` bump when its `peerDependencies` changed. Packages that consume an updated catalog entry via the `catalog:` protocol are included. Private packages, packages without a name, and packages listed in the `ignore` array of `.changeset/config.json` are skipped. If `.changeset/config.json` does not exist, a warning is printed and no changeset is generated.

Set [`update.changeset`](../settings/dependency-resolution.md#updatechangeset) to `true` in `pnpm-workspace.yaml` to enable this behavior by default, and use `--no-changeset` to override the setting for one update.

### --include-github-actions

Added in: v11.16.0

Also update the GitHub Actions referenced by the repository's workflow files. See [Updating GitHub Actions](#updating-github-actions).

### Supply-chain policy flags

Added in: v11.26.0

`pnpm update` accepts the same policy overrides as `pnpm install` and `pnpm add`: `--trust-lockfile`, `--no-trust-lockfile`, [`--trust-policy`](../settings/dependency-resolution.md#trustpolicy), [`--trust-policy-exclude`](../settings/dependency-resolution.md#trustpolicyexclude), and [`--trust-policy-ignore-after`](../settings/dependency-resolution.md#trustpolicyignoreafter), so a policy can be relaxed or tightened for a single run without editing `pnpm-workspace.yaml`.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
