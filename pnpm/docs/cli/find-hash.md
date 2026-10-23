---
id: find-hash
title: "pnpm find-hash"
---

:::warning

This command is experimental

:::

Lists the packages that include the file with the specified hash. The hash is either in `sha512-<base64>` form or a hex digest, as printed by [`pnpm cat-index`](./cat-index.md). For example:

```sh
pnpm find-hash sha512-mvavhfVcEREI7d8dfvfvIkuBLnx7+rrkHHnPi8mpEDUlNpY4CUY+CvJ5mrrLl18iQYo1odFwBV7z/cOypG7xxQ==
```

Each match is printed on its own line as `<name>@<version>` followed by its key in the store index.
