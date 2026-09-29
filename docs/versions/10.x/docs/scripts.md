---
id: scripts
title: Scripts
---

How pnpm handles the `scripts` field of `package.json`.

## Lifecycle Scripts

### `pnpm:devPreinstall`

Runs when `pnpm install` runs in the project itself, including in CI. It does not run when the package is installed as a dependency of another project.

Runs before any dependency is installed.

This script is executed only when set in the root project's `package.json`.
