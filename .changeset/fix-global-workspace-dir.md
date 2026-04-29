---
"@pnpm/config": patch
"pnpm": patch
---

**fix**: `--global` installs load `onlyBuiltDependencies` from `pnpm-workspace.yaml` in the global package directory.

The `workspaceDir` field is deleted during `--global` installs to prevent reading from the current working directory, but there was no fallback to read the workspace manifest from `globalPkgDir`. As a result, `onlyBuiltDependencies` from a global `pnpm-workspace.yaml` was silently ignored, causing all build scripts to be skipped by default in pnpm v10.

This fix adds an `else if (cliOptions['global'])` branch that reads the workspace manifest from `globalPkgDir`, ensuring global installs respect the build policy configured in the global `pnpm-workspace.yaml`.

Fixes #9073, #9478.
