---
id: pnpm-run
title: pnpm run
original_id: pnpm-run
---

Runs a defined package script.

```
pnpm run <command> [-- <args>...]
```

In addition to the shell’s pre-existing `PATH`, `pnpm run` adds `node_modules/.bin`
to the `PATH` provided to `scripts`. As of v3.5, when executed inside a workspace,
`<workspace root>/node_modules/.bin` is also added to the `PATH`, so if a tool
is installed in the workspace root, it may be called in any workspace package's `scripts`.
