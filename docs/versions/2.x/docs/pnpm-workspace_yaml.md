---
id: pnpm-workspace_yaml
title: pnpm-workspace.yaml
---

`pnpm-workspace.yaml` defines the root of the [workspace] and enables you to
include / exclude directories from the workspace. By default, all packages of
all subdirectories are included.

For example:

```yaml
packages:
  # all packages in subdirs of packages/ and components/
  - 'packages/**'
  - 'components/**'
  # exclude packages that are inside test directories
  - '!**/test/**'
```

The root package is always included, even when custom location wildcards are
used.

[workspace]: workspaces.md
