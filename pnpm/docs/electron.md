---
id: electron
title: Working with Electron
---

## electron-builder

electron-builder works with pnpm's default settings, including workspaces and
the [global virtual store](./global-virtual-store.md).

## Electron Forge

Electron Forge only packages a flat `node_modules`. Set
[`nodeLinker`](./settings/node-modules.md#nodelinker) in
`pnpm-workspace.yaml`. In a workspace, also set
[`hoistingLimits`](./settings/node-modules.md#hoistinglimits), so each
project's dependencies stay in its own `node_modules`, where Forge looks for
them:

```yaml title="pnpm-workspace.yaml"
nodeLinker: hoisted
hoistingLimits: workspaces
```

Then delete `node_modules` and run `pnpm install`.

Use the string form of `nodeLinker`. Forge rejects the object form.

The Forge error message suggests two other fixes. Neither works:

- `node-linker=hoisted` in `.npmrc` has no effect. pnpm does not read
  `nodeLinker` from `.npmrc`.
- A custom `publicHoistPattern` puts only one version of each package in the
  packaged app. A dependency that needs a different version gets the wrong one.
