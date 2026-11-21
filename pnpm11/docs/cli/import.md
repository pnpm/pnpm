---
id: import
title: pnpm import
---

`pnpm import` generates a `pnpm-lock.yaml` from another package manager's lockfile. Supported source files:
* `package-lock.json`
* `npm-shrinkwrap.json`
* `yarn.lock`

Yarn patch files must be inside the workspace. pnpm warns and imports the dependency without the patch if the file is outside it.

Declare workspaces in [pnpm-workspace.yaml](../settings.md) before running `pnpm import`.
