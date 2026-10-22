---
id: import
title: pnpm import
---

`pnpm import` generates a `pnpm-lock.yaml` from another package manager's lockfile. Supported source files, in the order pnpm looks for them:
* `yarn.lock`
* `package-lock.json`
* `npm-shrinkwrap.json`

```sh
pnpm import
```

The import is not a one-to-one conversion. The versions in the source lockfile are preferred during a fresh resolution, so the result can differ where the source lockfile does not satisfy the manifests. In a workspace, the `yarn.lock` files of individual projects are read too. Dependencies that use Yarn's `patch:` protocol become [`patchedDependencies`](./patch.md#patcheddependencies) entries.

An existing `pnpm-lock.yaml` is replaced. If the import fails, the previous file is restored.

Without a supported lockfile, the command fails with `ERR_PNPM_LOCKFILE_NOT_FOUND`. A `yarn.lock` that cannot be parsed, including one with merge conflict markers, fails with `ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED`.

Declare workspaces in [pnpm-workspace.yaml](../workspaces.md) before running `pnpm import`.
