---
id: remove
title: pnpm remove
original_id: remove
---

Removes packages from `node_modules` and from the project's `packages.json`.

```text
pnpm remove [-r] [--filter <package_selector>] <pkg>...
pnpm recursive remove [--filter <package_selector>] <pkg>...
pnpm multi remove [--filter <package_selector>] <pkg>...
```

Aliases: rm, r, uninstall, un

## Options

### --recursive, -r

When used inside a [workspace](../workspaces.md), removes a dependency (or dependencies)
from every workspace package.

When used not inside a workspace, removes a dependency (or dependencies)
from every package found in subdirectories.

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)

### --global

Remove a global package.
