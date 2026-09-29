---
id: approve-builds
title: pnpm approve-builds
---

Added in: v10.1.0

Approve dependencies for running scripts during installation.

The approved dependencies are added to the [`onlyBuiltDependencies`] array in `pnpm-workspace.yaml`, while unapproved ones are saved to [`ignoredBuiltDependencies`]. You can also update these settings manually if you prefer.

[`onlyBuiltDependencies`]: ../settings.md#onlybuiltdependencies
[`ignoredBuiltDependencies`]: ../settings.md#ignoredbuiltdependencies

## Options

### --all

Added in: v10.32.0

Approve all pending builds without interactive prompts.

### --global, -g

Added in: v10.4.0

Approve dependencies of globally installed packages.
