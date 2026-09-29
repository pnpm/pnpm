---
id: import
title: pnpm import
---

Added in: v2.15.0

`pnpm import` generates a `pnpm-lock.yaml` from another package manager's lockfile. Supported source files:
* `package-lock.json`
* `npm-shrinkwrap.json`
* `yarn.lock` (since v6.14.0)

Note that if you have workspaces you wish to import dependencies for, they will need to be declared in a [pnpm-workspace.yaml](../pnpm-workspace_yaml.md) file beforehand.
