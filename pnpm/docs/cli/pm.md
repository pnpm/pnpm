---
id: pm
title: pnpm pm
---

The `pnpm pm <command>` syntax always runs the built-in pnpm command, bypassing any same-named script in `package.json`.

Some built-in commands can be overridden by scripts. For example, if your project defines a `"clean"` script in `package.json`, then `pnpm clean` runs that script instead of the built-in [`pnpm clean`](./clean.md). Using `pnpm pm clean` forces the built-in command to run.

## Example

```json title="package.json"
{
  "scripts": {
    "clean": "rm -rf dist"
  }
}
```

```sh
# Runs the "clean" script from package.json
pnpm clean
# or explicitly:
pnpm run clean

# Runs the built-in pnpm clean command (removes node_modules)
pnpm pm clean
```
