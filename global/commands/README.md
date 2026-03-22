# @pnpm/global.commands

Command handlers for pnpm's global package management (`pnpm add -g`, `pnpm remove -g`, `pnpm update -g`, `pnpm list -g`).

## Architecture: Isolated Global Packages

Unlike npm, where all global packages share a single `node_modules/` directory, pnpm installs each global package (or group of packages installed together) into its own **isolated installation directory**. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

### Directory layout

All global package data lives under `{pnpmHomeDir}/global/v11/` (the "global package directory", referred to as `globalPkgDir` in the code). The layout is:

```
{pnpmHomeDir}/global/v11/
  {hash-1}          -> symlink to {pid}-{timestamp-1}/   (hash symlink)
  {pid}-{timestamp-1}/                                    (install dir)
    package.json
    node_modules/
      .pnpm/
      {pkg-a}/
      {pkg-b}/
    pnpm-lock.yaml
  {hash-2}          -> symlink to {pid}-{timestamp-2}/
  {pid}-{timestamp-2}/
    package.json
    node_modules/
      .pnpm/
      {pkg-c}/
    pnpm-lock.yaml
```

Each install group has two entries:

1. **Install directory** (`{pid}-{timestamp}`): A regular directory containing a complete pnpm project with its own `package.json`, `node_modules/`, and lockfile. Named with the process ID and timestamp at creation time to avoid collisions.

2. **Hash symlink** (`{hash}`): A symbolic link named with a deterministic hash of the package aliases and registries. Points to the install directory. This serves as the lookup key for finding where a set of packages is installed.

The hash is computed from the sorted list of package aliases and sorted registry URLs, making it deterministic for a given set of packages.

### How each command works

#### `pnpm add -g <pkg> [pkg2 ...]`

Handled by `handleGlobalAdd()`:

1. Clean up orphaned install directories (those not referenced by any hash symlink).
2. Create a new install directory `{pid}-{timestamp}`.
3. Run `installGlobalPackages()` to install the requested packages into this directory using `@pnpm/core`'s `mutateModulesInSingleProject()`.
4. Read the resolved aliases from the resulting `package.json` (this is more reliable than parsing aliases from CLI params, which may be tarballs, git URLs, etc.).
5. Check for bin name conflicts with existing global packages via `checkGlobalBinConflicts()`.
6. Remove any existing global installations of the same aliases.
7. Create a hash symlink pointing to the new install directory.
8. Link bins from the installed packages into the global bin directory.

#### `pnpm remove -g <pkg> [pkg2 ...]`

Handled by `handleGlobalRemove()`:

1. Look up each requested package alias to find its install group (via `findGlobalPackage()`).
2. For each affected group: remove all bin shims, delete the hash symlink, and delete the install directory.

#### `pnpm update -g [pkg ...]`

Handled by `handleGlobalUpdate()`:

1. Scan all existing global packages.
2. Filter to groups containing the requested packages (or all groups if no args).
3. For each group:
   - Create a new install directory.
   - Re-install the same packages (at `--latest` versions if the `--latest` flag is set, or within existing ranges otherwise).
   - Check for bin conflicts, remove old bins, swap the hash symlink to point to the new directory, clean up the old directory, and link new bins.

#### `pnpm list -g [pattern ...]`

Handled by `listGlobalPackages()`:

1. Scan all global packages.
2. Read package details (alias, version) from each install group's `node_modules/`.
3. Filter by glob patterns if provided (via `@pnpm/matcher`).
4. Sort and display.

### `installGlobalPackages()` — the core install function

This is a focused ~30-line function that replaces the 450-line `installDeps()` from `plugin-commands-installation`. Global installs don't need workspace logic, recursive installs, update matching, rebuild orchestration, or any of the other complexity in `installDeps()`. The function does exactly:

1. Create a store controller.
2. Read (or create) a manifest for the install directory.
3. Call `mutateModulesInSingleProject()` with `mutation: 'installSome'`.
4. Write the updated manifest.

### Bin conflict detection

`checkGlobalBinConflicts()` prevents a common footgun: installing a global package whose binaries would shadow binaries from a different globally-installed package. Before any bin linking happens, it:

1. Collects bin names from the packages about to be installed.
2. Checks if any of those bin names already exist in the global bin directory.
3. If they do, verifies whether they belong to a package being replaced (ok) or to a different package (error).

### Orphan cleanup

`cleanOrphanedInstallDirs()` (from `@pnpm/global.packages`) runs at the start of `add` and `update` to remove install directories that are no longer referenced by any hash symlink. This handles cases where a previous install was interrupted or crashed. A 5-minute safety window prevents cleaning up directories from concurrent installs that haven't created their hash symlink yet.

## Package structure

```
global/
  commands/           @pnpm/global.commands (this package)
    src/
      globalAdd.ts          handleGlobalAdd()
      globalRemove.ts       handleGlobalRemove()
      globalUpdate.ts       handleGlobalUpdate()
      listGlobalPackages.ts listGlobalPackages()
      installGlobalPackages.ts  core install function
      checkGlobalBinConflicts.ts  bin conflict detection
      readInstalledPackages.ts    shared helper
      index.ts
  packages/           @pnpm/global.packages
    src/
      scanGlobalPackages.ts   directory scanning, package lookup
      globalPackageDir.ts     install dir / hash link management
      cacheKey.ts             deterministic hash computation
      index.ts
```

`@pnpm/global.packages` provides the low-level utilities for reading and managing the directory structure. `@pnpm/global.commands` provides the high-level command handlers that orchestrate installs, removals, updates, and listing.

## Integration points

The CLI command handlers in `plugin-commands-installation` and `plugin-commands-listing` delegate to this package with a simple early return:

```typescript
// In add.ts handler:
if (opts.global) {
  return handleGlobalAdd(opts, params)
}

// In remove.ts handler:
if (opts.global) {
  return handleGlobalRemove(opts, params)
}

// In update/index.ts handler:
if (opts.global) {
  return handleGlobalUpdate(opts, params)
}

// In list.ts handler:
if (opts.global && opts.globalPkgDir) {
  return listGlobalPackages(opts.globalPkgDir, params)
}
```
