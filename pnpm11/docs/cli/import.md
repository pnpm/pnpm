---
id: import
title: pnpm import
---

`pnpm import` generates a `pnpm-lock.yaml` from another package manager's lockfile. Supported source files:
* `package-lock.json`
* `npm-shrinkwrap.json`
* `yarn.lock`

Declare workspaces in [pnpm-workspace.yaml](../settings.md) before running `pnpm import`.
