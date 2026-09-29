---
id: run
title: pnpm run
original_id: run
---

Aliases: `run-script`

Runs a defined package script.

## Synopsis

```text
pnpm run [[-r] [--no-bail] [--no-sort] [--workspace-concurrency=<number>]]
     <command> [-- <args>...]

pnpm multi run <command> [-- <args>...]

pnpm recursive run <command> [-- <args>...]
```

In addition to the shell’s pre-existing `PATH`, `pnpm run` adds `node_modules/.bin`
to the `PATH` provided to `scripts`. As of v3.5, when executed inside a workspace,
`<workspace root>/node_modules/.bin` is also added to the `PATH`, so if a tool
is installed in the workspace root, it may be called in any workspace package's `scripts`.

## Options

### --recursive, -r

This runs an arbitrary command from each package's "scripts" object.
If a package doesn't have the command, it is skipped.
If none of the packages have the command, the command fails.

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)
