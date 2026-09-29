---
id: doctor
title: pnpm doctor
---

Added in: v11.14.0

Runs diagnostics on the pnpm installation and the environment it runs in.

```sh
pnpm doctor [--offline] [--benchmark] [--json]
```

Each check reports how to fix what it finds, and the command exits with a non-zero code when any check fails. Warnings do not fail the command.

```
✓ Versions: pnpm 12.3.4, Node.js 22.20.0
✓ Install method: pnpm
✓ Global bin directory: /Users/example/Library/pnpm/bin
✓ Cache directory: /Users/example/Library/Caches/pnpm
✓ Store directory: /Users/example/Library/pnpm/store/v10
✓ Filesystem: available: reflink, hardlink, symlink
✓ Registry connectivity: https://registry.npmjs.org/ (128ms)
✓ Install smoke test: offline "file:" install linked its dependency

All checks passed
```

## Checks

### Versions

Reports the running pnpm and Node.js versions.

### Install method

Reports how pnpm was installed — as the `pnpm` package or the `@pnpm/exe` standalone build — and warns when pnpm is being run by Corepack, which manages the pnpm version itself and makes `pnpm self-update` unavailable.

### Global bin directory

Checks that the directory pnpm links global executables into is on `PATH` and writable. If it is missing from `PATH`, the fix is to run [`pnpm setup`](./setup.md).

### Cache directory

Checks that the [cache directory](../settings/other.md#cachedir) is writable.

### Store directory

Checks that the [store directory](../settings/store.md#storedir) is writable. Skipped when no store directory is configured.

### Filesystem

Probes which link strategies work from the store's volume: reflink (copy-on-write), hardlink, and symlink. This is what determines how packages land in `node_modules` and how fast an install is — a reflink or hardlink is near-free, a plain copy is not.

If neither reflink nor hardlink works, the check warns that installs will fall back to copying, and suggests putting the store on the same filesystem as your projects.

### Registry connectivity

Pings the configured registry with a 15-second timeout and reports the round-trip time. Fails if the registry cannot be reached or answers with an error status, which usually points at network, proxy, or auth configuration.

Skipped with `--offline`.

### Install smoke test

Installs a throwaway package as a `file:` dependency, entirely offline, in a temporary directory. This exercises the resolve, store, and link path end to end and confirms the running binary can actually perform an install.

This check is always offline by construction, so `--offline` does not skip it.

## Options

### --offline

Skip the checks that need network access.

### --benchmark

Also time the filesystem and install checks, reporting the duration alongside each result.

### --json

Report the results as JSON. The output is an object with a single `checks` array, each entry having `title`, `status` (`pass`, `warn`, or `fail`), and optionally `detail`, `fix`, and `durationMs`.

```json
{
  "checks": [
    {
      "title": "Filesystem",
      "status": "pass",
      "detail": "available: reflink, hardlink, symlink",
      "durationMs": 3
    }
  ]
}
```
