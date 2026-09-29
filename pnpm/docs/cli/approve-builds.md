---
id: approve-builds
title: pnpm approve-builds
---

Added in: v10.1.0

Approve dependencies for running scripts during installation.

The approved dependencies are added to the [`allowBuilds`] map in `pnpm-workspace.yaml` with a value of `true`, while unapproved ones are saved with a value of `false`. You can also update these settings manually if you prefer.

[`allowBuilds`]: ../settings/build.md#allowbuilds

## Usage

You can run `pnpm approve-builds` without arguments to get an interactive prompt, or pass package names as positional arguments:

```sh
pnpm approve-builds esbuild fsevents !core-js
```

Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched.

Since v12.4.0, a decision is saved even when nothing is awaiting approval, so a policy can be written before the package is installed. A named package that is not awaiting approval is reported with a warning rather than an error, and is not rebuilt: it may not be installed yet, and rebuilding it would need a lockfile the project may not have. An argument that names no package at all is rejected before anything is written.

During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added to `pnpm-workspace.yaml` with a placeholder value, so you can manually set them to `true` or `false`.

Since v11.23.0, writing `allowBuilds` also removes `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies` from `pnpm-workspace.yaml`. `allowBuilds` replaced those settings in pnpm 11 and they have been ignored since, so a workspace migrated from pnpm 10 kept them around looking active.

## Options

### --all

Added in: v10.32.0

Approve all pending builds without interactive prompts.

### --global, -g

Approve builds for [globally installed packages](../global-packages.md).

pnpm gathers the packages awaiting approval across **every** [isolated install
group](../global-packages.md#isolated-installations), asks about them once, and
writes a single policy into the `pnpm-workspace.yaml` of the global packages
directory. Only the groups that actually contain an approved package are then
rebuilt.

:::info Removed in v11.0.0, back in v11.24.0

Isolated global installs gave every install group its own manifest, and
`pnpm approve-builds -g` was removed rather than made to guess which one to
write to. Since v11.24.0 it writes one policy for all of them, so it works
again. In between — and still, if you would rather decide at install time — use
`--allow-build` when installing globally (e.g.
`pnpm add -g --allow-build=esbuild esbuild`), or answer the prompt pnpm shows
during a global install.

:::
