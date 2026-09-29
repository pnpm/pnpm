---
id: scripts
title: Scripts
---

How pnpm handles the `scripts` field of `package.json`.

## Lifecycle Scripts

### `pnpm:devPreinstall`

Added in: v6.21.0

Runs only on local `pnpm install`.

Runs before any dependency is installed.

This script is executed only when set in the root project's `package.json`.
