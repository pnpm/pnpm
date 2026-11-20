---
id: prefix
title: pnpm prefix
---

Added in: v11.10.0

Prints the current package prefix directory: the nearest directory, starting from the current one, that contains `node_modules`, `package.json`, `package.json5`, `package.yaml`, or `pnpm-workspace.yaml`. If no such directory is found, the current directory is printed.

```sh
pnpm prefix
```

## Options

### --global, -g

Prints the global prefix directory instead.
