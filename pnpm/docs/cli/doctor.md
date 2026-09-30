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
✓ Node.js on PATH: /Users/example/.local/share/fnm/node-versions/v22.20.0/installation/bin/node
✓ Script shell: scripts run in /bin/sh (/bin/bash), package executables in /bin/sh (/bin/bash)
✓ Global bin directory: /Users/example/Library/pnpm/bin
✓ Cache directory: /Users/example/Library/Caches/pnpm
✓ Store directory: /Users/example/Library/pnpm/store/v10
✓ Filesystem: available: reflink, hardlink, symlink
✓ Registry connectivity: https://registry.npmjs.org/ (128ms)
✓ Install smoke test: offline "file:" install linked its dependency
✓ Lifecycle scripts: an install script ran /Users/example/.local/share/fnm/node-versions/v22.20.0/installation/bin/node, a script in /Users/example/my-project ran /Users/example/.local/share/fnm/node-versions/v22.20.0/installation/bin/node

All checks passed
```

## Checks

### Versions

Reports the running pnpm and Node.js versions.

### Install method

Reports how pnpm was installed — as the `pnpm` package or the `@pnpm/exe` standalone build — and warns when pnpm is being run by Corepack, which manages the pnpm version itself and makes `pnpm self-update` unavailable.

### Node.js on PATH

Added in: v12.9.0

Lists the `node` executables on `PATH` in lookup order. Lifecycle scripts run the first one. pnpm itself does not need Node.js, but the executables that packages put in `node_modules/.bin` do.

Warns when there is no `node` on `PATH`, and when a `node` entry is skipped because it is a broken link, not executable, or not a file. A version manager that relinks `node` while scripts run can leave such an entry behind.

### Script shell

Added in: v12.9.0

Reports the shell that runs scripts: the [`scriptShell`](../settings/other.md#scriptshell), the built-in shell emulator, or `sh`. Outside Windows it also reports what `/bin/sh` is, because `/bin/sh` starts every executable in `node_modules/.bin`. On macOS that is the shell `/private/var/select/sh` links to.

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

### Lifecycle scripts

Added in: v12.9.0

Installs a temporary project whose `postinstall` script calls a dependency's executable, then runs the same executable through `pnpm exec` in the current project, if there is one. Both go through the executable's `node_modules/.bin` shim, which has to find `node` on `PATH`. The check reports the `node` each run used.

If a run fails, the report includes the error and, outside Windows, the end of an `sh -x` trace of the shim. The trace shows the `PATH` the shim searched and which `node`, if any, it started. A shim that finds no `node` is the usual cause of a script failing with exit status 127.

The run in the current project goes through `pnpm exec`, so the project's pnpmfile and `nodeOptions` apply to it as they do to `pnpm exec`. It does not install the project's dependencies. The whole check is skipped when there is no `node` on `PATH`.

## Options

### --offline

Skip the checks that need network access.

### --benchmark

Also time the filesystem, install, and lifecycle script checks, reporting the duration alongside each result.

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
