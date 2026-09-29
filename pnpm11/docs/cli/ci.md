---
id: ci
title: pnpm ci
---

Added in: v11.0.0

Aliases: `clean-install`, `ic`, `install-clean`

Perform a clean install. This command runs [`pnpm clean`](./clean.md) followed by [`pnpm install --frozen-lockfile`](./install.md).

Designed for CI/CD environments where reproducible builds are critical.

```sh
pnpm ci
```
