---
id: pnpm-workspace_yaml
title: pnpm-workspace.yaml
---

`pnpm-workspace.yaml` defines the root of the [workspace] and enables you to
include / exclude directories from the workspace. By default, all packages of
all subdirectories are included.

For example:

```yaml title="pnpm-workspace.yaml"
packages:
  # all packages in direct subdirs of packages/
  - 'packages/*'
  # all packages in subdirs of components/
  - 'components/**'
  # exclude packages that are inside test directories
  - '!**/test/**'
```

The root package is always included, even when custom location wildcards are
used.

[workspace]: workspaces.md
