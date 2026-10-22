---
id: peers
title: pnpm peers
---

Checks for unmet and missing peer dependency issues by reading the lockfile. pnpm reads the lockfile of the last install in `node_modules` when there is one, and `pnpm-lock.yaml` otherwise.

## Commands

### check

```sh
pnpm peers check
```

Reports any packages with unresolved or incompatible peer dependencies. A bare `pnpm peers` runs `check`.

The command exits with code 1 when it finds issues.

## Options

### --json

Print the issues in JSON format.

### --lockfile-only

Read `pnpm-lock.yaml` even when `node_modules` holds the lockfile of the last install.

### --recursive, -r

Check every workspace project. Combine it with [`--filter`](../filtering.md) to select the projects to check.
