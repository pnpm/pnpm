---
id: electron
title: Working with Electron
---

## Approve build scripts

pnpm does not run dependency build scripts until you approve them in
[`permissions`](./settings/build.md#permissions).

Electron 42 and later download the Electron binary the first time it runs.
They have no build script to approve. Electron 41 and earlier download it in a
`postinstall` script, so approve it:

```yaml title="pnpm-workspace.yaml"
permissions:
  electron:
    build: true
```

Approve native addons, such as `better-sqlite3`, the same way. electron-builder
and Electron Forge rebuild them for Electron when they package the app.

electron-builder depends on `electron-winstaller`, which has an install script
that only Squirrel.Windows builds need. Approve it if you build that target.
Otherwise, deny it:

```yaml title="pnpm-workspace.yaml"
permissions:
  electron-winstaller:
    build: false
```

## electron-builder

electron-builder reads the dependency tree from `pnpm list` and copies the
production dependencies into the app. It works with the default `isolated`
layout and with the [global virtual store](./global-virtual-store.md), in a
single project and in a workspace. No pnpm settings are needed. We tested
it with electron-builder 26.18.

## Electron Forge

Electron Forge only packages a flat `node_modules`. Before packaging, it checks
that pnpm uses the `hoisted` linker and fails otherwise. Set
[`nodeLinker`](./settings/node-modules.md#nodelinker) in
`pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
nodeLinker: hoisted
```

The Forge error message suggests adding `node-linker=hoisted` to `.npmrc`.
pnpm does not read `nodeLinker` from `.npmrc`, so that line has no effect.
Use the string form shown above. Forge rejects the object form of
`nodeLinker`.

In a workspace, the `hoisted` linker moves dependencies to the
`node_modules` of the workspace root, where Forge does not look for them. Keep
each project's dependencies in its own `node_modules` with
[`hoistingLimits`](./settings/node-modules.md#hoistinglimits):

```yaml title="pnpm-workspace.yaml"
nodeLinker: hoisted
hoistingLimits: workspaces
```

After changing these settings, run `pnpm install` to relink `node_modules`. If
`node_modules` was already installed with the `hoisted` linker, delete it
first. pnpm does not relink a hoisted `node_modules` when only
`hoistingLimits` changes.

The Forge error message also accepts a custom `hoistPattern` or
`publicHoistPattern` in place of the `hoisted` linker. Do not rely on it. With
`publicHoistPattern: ['*']`, only one version of each package reaches the
root of `node_modules`. A dependency that needs a different version gets the
wrong one in the packaged app.
