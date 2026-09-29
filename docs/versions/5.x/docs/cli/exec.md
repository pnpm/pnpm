---
id: exec
title: pnpm exec
---

Shorthand form of `pnpm -r exec`.

See the [`-r exec`] documentation for more information.

[`r exec`]: recursive#pnpm--r-exec

## Options

### --parallel

Disregards concurrency and topological sorting configuration entirely and runs
the command immediately in all matching packages, with prefixed streaming
output.

This is the preferred option for processes that take a long time to run.
For example, running a build process with the watch flag over a large number of
packages.
