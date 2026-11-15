---
id: ci
title: pnpm ci
---

Added in: v11.0.0

Aliases: `clean-install`, `ic`, `install-clean`

Perform a clean install. `pnpm ci` validates that `pnpm-lock.yaml` exists and is up to date before removing `node_modules`, then runs [`pnpm clean`](./clean.md) followed by [`pnpm install --frozen-lockfile`](./install.md).

Designed for CI/CD environments where reproducible builds are critical.

```sh
pnpm ci
```

## Options

`pnpm ci` accepts the options of [`pnpm install`](./install.md), such as `--prod`, `--offline`, and `--filter`.
