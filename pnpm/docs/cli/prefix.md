---
id: prefix
title: pnpm prefix
---

Prints the current package prefix directory: the nearest directory, starting from the current one, that contains `node_modules`, `package.json`, `package.json5`, `package.yaml`, `pnpm-workspace.yaml`, `Cargo.toml`, or `pyproject.toml`. If no such directory is found, the current directory is printed.

```sh
pnpm prefix
```

## Options

### --global, -g

Prints the global prefix directory instead: the directory that holds the global packages. pnpm creates the global bin directory if it is missing. The command fails with `ERR_PNPM_NO_GLOBAL_BIN_DIR` if no global bin directory is configured. Run [`pnpm setup`](./setup.md) to configure one.
