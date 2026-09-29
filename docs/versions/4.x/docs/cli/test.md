---
id: test
title: pnpm test
original_id: test
---

Aliases: `run test`, `t`, `tst`

Runs a package's `"test"` script, if one was provided.
This is just a shortcut to `pnpm run test`, so for more details you
may read about [pnpm run](run.md).

## Synopsis

```text
pnpm test [-r] [-- <args>...]
```

## Options

### --recursive, -r

Run the tests in every package found in subdirectories
or every workspace package, when executed inside a workspace.

### --filter &lt;package_selector>

[Read more about filtering.](../filtering.md)
