---
id: install-test
title: pnpm install-test
---

Aliases: `it`

Runs `pnpm install` followed immediately by `pnpm test`. It takes exactly the
same arguments as [`pnpm install`](./install.md).

Arguments after `--` are passed to the `test` script:

```sh
pnpm it -- --watch
```
