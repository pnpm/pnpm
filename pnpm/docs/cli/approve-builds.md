---
id: approve-builds
title: pnpm approve-builds
---

Approve dependencies for running scripts during installation.

The approved dependencies are added to [`permissions`] in `pnpm-workspace.yaml` with `build: true`, while unapproved ones are saved with `build: false`. You can also update these settings manually if you prefer.

Existing entries for other packages remain in [`allowBuilds`]. To review agent skills as well, use [`pnpm approve`](./permissions.md).

[`allowBuilds`]: ../settings/build.md#allowbuilds
[`permissions`]: ../settings/build.md#permissions

## Usage

You can run `pnpm approve-builds` without arguments to get an interactive prompt, or pass package names as positional arguments:

```sh
pnpm approve-builds esbuild fsevents !core-js
```

A bare name only approves default-registry dependencies. To approve a `file:`, git, or tarball dependency, use the full selector shown by `pnpm ignored-builds`. For example, run `pnpm approve-builds 'example@file:../example'`.

Running `pnpm approve-builds` without package arguments or `--all` when packages are awaiting approval requires an interactive terminal outside CI (`ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE`).

Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched. Approved packages that are awaiting approval are rebuilt right away. Naming a package both approved and denied fails with `ERR_PNPM_APPROVE_BUILDS_CONTRADICTING_ARGS`.

Since v12.4.0, a decision is saved even when nothing is awaiting approval, so a policy can be written before the package is installed. A named package that is not awaiting approval is reported with a warning rather than an error, and is not rebuilt: it may not be installed yet, and rebuilding it would need a lockfile the project may not have. An argument that names no package at all is rejected before anything is written.

During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added to `pnpm-workspace.yaml` with the placeholder value `set this to true or false`, so you can manually set them to `true` or `false`.

Writing a build decision also removes the ignored legacy settings `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies` from `pnpm-workspace.yaml`.

## Options

### --all

Approve all pending builds without interactive prompts. Cannot be combined with package names (`ERR_PNPM_APPROVE_BUILDS_ALL_WITH_ARGS`).

### --global, -g

Approve builds for [globally installed packages](../global-packages.md).

pnpm gathers the packages awaiting approval across **every** [isolated install
group](../global-packages.md#isolated-installations), asks about them once, and
writes the decisions to [`globalPackagesConfig.permissions`](../configuring.md#settings-for-global-packages)
in the global `config.yaml`. Only the groups that actually contain an approved
package are then rebuilt.
