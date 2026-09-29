---
id: link
title: pnpm link
---

Aliases: `ln`

Makes the current local package accessible system-wide, or in another location.

```text
pnpm link <dir>
pnpm link --global
pnpm link --global <pkg>
```

## Options

### --dir &lt;dir\>, -C

* **Default**: Current working directory
* **Type**: Path string

Changes the link location to `<dir>`.

### `pnpm link <dir>`

Links package from `<dir>` folder to node_modules of package from where you're executing this command or specified via `--dir` option.

### `pnpm link --global`

Links package from location where this command was executed or specified via `--dir` option to global `node_modules`, so it can be referred from another package with `pnpm link --global <pkg>`.

### `pnpm link --global <pkg>`

Links the specified package (`<pkg>`) from global `node_modules` to the `node_nodules` of package from where this command was executed or specified via `--dir` option.
