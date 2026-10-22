---
id: search
title: pnpm search
---

Aliases: `s`, `se`, `find`

Search the registry for packages matching the given keywords.

```sh
pnpm search <keyword> [<keyword> ...]
```

## Examples

```sh
pnpm search webpack plugin
pnpm search @types/node
```

## Options

### --json

Output search results in JSON format.

### --search-limit &lt;number\>

* Default: **20**

Maximum number of results to show.

### --registry &lt;url\>

The registry to search. Defaults to the configured default registry.
