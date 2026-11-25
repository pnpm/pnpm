---
id: env
title: "pnpm env <cmd>"
---

:::warning Deprecated

`pnpm env` is deprecated but remains available for users accustomed to its name. It is an alias for [`pnpm runtime`](./runtime.md). `pnpm env use <version>` aliases `pnpm runtime set node <version>`.

:::

`pnpm env list` remains available with its existing behavior. `pnpm env remove <version>` aliases `pnpm runtime unset node <version>`.

```sh
pnpm env list [<version>]
pnpm env remove <version> [-g]
```
