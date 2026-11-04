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

The command exits with code 1 when it finds unresolved or incompatible peers.

With `autoInstallPeers: false` and `dedupePeerDependents: true`, the command also warns about resolved required peers that the project and its dependency ancestors do not declare. Dependencies of the workspace root count as providers when `resolvePeersFromWorkspaceRoot` is enabled. Optional peers do not produce these warnings.

These warnings describe the project's declarations, even when a dependency carries its own fallback copy of the peer. They are advisory and do not change the exit code or cause `strictPeerDependencies` failures. Declare the peer directly if your project's scripts use it. JSON output includes these warnings in each project's `undeclared` array.

## Options

### --global, -g

Checks peer dependency issues in globally installed packages.

### --json

Print the issues in JSON format.

### --lockfile-only

Read `pnpm-lock.yaml` even when `node_modules` holds the lockfile of the last install.

### --recursive, -r

Check every workspace project. Combine it with [`--filter`](../filtering.md) to select the projects to check.
