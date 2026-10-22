---
id: approve-builds
title: pnpm approve-builds
---

Approve dependencies for running scripts during installation.

The approved dependencies are added to the [`allowBuilds`] map in `pnpm-workspace.yaml` with a value of `true`, while unapproved ones are saved with a value of `false`. You can also update these settings manually if you prefer.

Since v12.11.0, when `pnpm-workspace.yaml` already has a [`permissions`] setting, the decisions are written there as the `build` capability instead. To review agent skills as well, use [`pnpm approve`](./permissions.md).

[`allowBuilds`]: ../settings/build.md#allowbuilds
[`permissions`]: ../settings/build.md#permissions

## Usage

You can run `pnpm approve-builds` without arguments to get an interactive prompt, or pass package names as positional arguments:

```sh
pnpm approve-builds esbuild fsevents !core-js
```

Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched. Approved packages that are awaiting approval are rebuilt right away. Naming a package both approved and denied fails with `ERR_PNPM_APPROVE_BUILDS_CONTRADICTING_ARGS`.

Since v12.4.0, a decision is saved even when nothing is awaiting approval, so a policy can be written before the package is installed. A named package that is not awaiting approval is reported with a warning rather than an error, and is not rebuilt: it may not be installed yet, and rebuilding it would need a lockfile the project may not have. An argument that names no package at all is rejected before anything is written.

During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added to `pnpm-workspace.yaml` with the placeholder value `set this to true or false`, so you can manually set them to `true` or `false`.

Writing `allowBuilds` also removes the ignored legacy settings `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies` from `pnpm-workspace.yaml`.

## Options

### --all

Approve all pending builds without interactive prompts. Cannot be combined with package names (`ERR_PNPM_APPROVE_BUILDS_ALL_WITH_ARGS`).

### --global, -g

Approve builds for [globally installed packages](../global-packages.md).

pnpm gathers the packages awaiting approval across **every** [isolated install
group](../global-packages.md#isolated-installations), asks about them once, and
writes a single policy into the `pnpm-workspace.yaml` of the global packages
directory. Only the groups that actually contain an approved package are then
rebuilt.
