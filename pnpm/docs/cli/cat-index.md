---
id: cat-index
title: "pnpm cat-index"
---

Prints the index file of a specific package from the store. The package is specified by its name and version:

```sh
pnpm cat-index <pkg name>@<pkg version>
```

The output is JSON that lists each file of the package with its `digest`, `mode`, and `size`. To find the packages that contain a file, pass its hash to [`pnpm find-hash`](./find-hash.md).
