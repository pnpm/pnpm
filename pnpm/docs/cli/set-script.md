---
id: set-script
title: pnpm set-script
---

Added in: v11.3.0

Aliases: `ss`

Adds or updates an entry in the `scripts` field of the project manifest.

```sh
pnpm set-script <name> <command>
```

Supports `package.json`, `package.json5`, and `package.yaml` manifest formats.

If the `scripts` field does not exist, it is created. If a script with the same name already exists, it is overwritten.

## Examples

```sh
pnpm set-script test "vitest run"
pnpm set-script build "tsc -p ."
pnpm ss lint "eslint ."
```

The above is equivalent to manually editing `package.json`:

```json
{
  "scripts": {
    "test": "vitest run",
    "build": "tsc -p .",
    "lint": "eslint ."
  }
}
```

## See also

- [`pnpm pkg set`](./pkg.md#set) — set arbitrary fields in `package.json`, including individual scripts via `scripts.<name>=<command>`.
