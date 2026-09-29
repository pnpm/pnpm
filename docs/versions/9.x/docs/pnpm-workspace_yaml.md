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
  # specify a package in a direct subdir of the root
  - 'my-app'
  # all packages in direct subdirs of packages/
  - 'packages/*'
  # all packages in subdirs of components/
  - 'components/**'
  # exclude packages that are inside test directories
  - '!**/test/**'
```

The root package is always included, even when custom location wildcards are
used.

Catalogs are also defined in the `pnpm-workspace.yaml` file. See [_Catalogs_](./catalogs.md) for details.

```yaml title="pnpm-workspace.yaml"
packages:
  - 'packages/*'

catalog:
  chalk: ^4.1.2

catalogs:
  react16:
    react: ^16.7.0
    react-dom: ^16.7.0
  react17:
    react: ^17.10.0
    react-dom: ^17.10.0
```

[workspace]: workspaces.md
