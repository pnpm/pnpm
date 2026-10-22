---
id: dedupe
title: "pnpm dedupe"
---

Perform an install removing older dependencies in the lockfile if a newer version can be used.

## Workspace selection

Since v12.4.1, `pnpm dedupe` processes every project in the workspace by default, including workspaces that keep [a lockfile per project](../workspaces.md#sharedworkspacelockfile). Narrow the run with the usual [filters](../filtering.md), for example `pnpm dedupe --filter ./packages/app`.

## Options

### `--check`

Check if running dedupe would result in changes without installing packages or editing the lockfile. Exits with a non-zero status code if changes are possible.

### `--lockfile-only`

Only update `pnpm-lock.yaml`. Packages are not downloaded and `node_modules` is not written.

### `--ignore-scripts`

Don't run lifecycle scripts of the project or its dependencies.

### `--ignore-pnpmfile`

Don't run the hooks of the [pnpmfile](../pnpmfile.md).

### `--offline`

Use only packages already in the store. A package missing from the store fails the command.

### `--prefer-offline`

Prefer cached packages and metadata over the network, even past their freshness window.

### `--no-runtime`

Skip installing runtime entries, such as Node.js downloaded via [`devEngines.runtime`](../package_json.md#devenginesruntime). Works like [`pnpm install --no-runtime`](./install.md#--no-runtime).

### `--filter <package_selector>`

Restrict the command to the selected projects. [Read more about filtering.](../filtering.md)

### `--fail-if-no-match`

Exit with an error when no project matches the given filters, instead of doing nothing.
