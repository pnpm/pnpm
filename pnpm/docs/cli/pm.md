---
id: pm
title: pnpm pm
---

The `pnpm pm <command>` syntax always runs the built-in pnpm command, bypassing any same-named script in `package.json`.

Some built-in commands can be overridden by scripts. For example, if your project defines a `"clean"` script in `package.json`, then `pnpm clean` runs that script instead of the built-in [`pnpm clean`](./clean.md). Using `pnpm pm clean` forces the built-in command to run.

These commands run a same-named script when one exists:

* [`clean`](./clean.md) and its alias `purge`
* [`rebuild`](./rebuild.md) and its alias `rb`
* [`deploy`](./deploy.md)
* [`setup`](./setup.md)
* [`tasks`](./tasks.md)

An alias looks up its own script name, so `pnpm rb` runs an `rb` script, not a `rebuild` script.

Inside a workspace, running one of these commands from a subdirectory fails with `ERR_PNPM_SCRIPT_OVERRIDE_IN_WORKSPACE_ROOT` when the root `package.json` has a script of that name. Run `pnpm pm <command>` to use the built-in command, or `pnpm run <command>` from the workspace root to run the script.

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

# Runs the built-in pnpm clean command
pnpm pm clean
```
