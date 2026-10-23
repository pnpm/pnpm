---
id: ci
title: pnpm ci
---

Aliases: `clean-install`, `ic`, `install-clean`

Perform a clean install. This command runs [`pnpm clean`](./clean.md) followed by [`pnpm install --frozen-lockfile`](./install.md).

Designed for CI/CD environments where reproducible builds are critical.

```sh
pnpm ci
```

## Options

`pnpm ci` accepts the options of [`pnpm install`](./install.md), such as `--prod`, `--offline`, and `--filter`.
