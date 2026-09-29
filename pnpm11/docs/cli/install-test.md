---
id: install-test
title: pnpm install-test
---

Aliases: `it`

Runs `pnpm install` followed immediately by `pnpm test`. It takes exactly the
same arguments as [`pnpm install`](./install.md).

## Options

### --no-bail

Added in: v11.28.0

In a recursive run (`pnpm -r install-test`), keep running the tests of the remaining workspace projects after one fails. See [`--no-bail`](./recursive.md#--no-bail).
