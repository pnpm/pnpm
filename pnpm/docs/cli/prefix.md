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

Prints the global prefix directory instead: the parent of the global packages directory that [`pnpm root -g`](./root.md) prints. pnpm creates the global bin directory if it is missing. The command fails with `ERR_PNPM_NO_GLOBAL_BIN_DIR` if no global bin directory is configured, and with `ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH` if the global bin directory is not in `PATH`. Run [`pnpm setup`](./setup.md) to configure one.
