# pnpm

## 5.2.9

### Patch Changes

- `run --silent <cmd>` should only print output of the command and nothing from pnpm (#2660).
- installing a new optional dependency that has an option dependency should not fail (#2663).

## 5.2.8

### Patch Changes

- Fixing some issues with finding the target directory for command files during global installation.

## 5.2.7

### Patch Changes

- Fixing some issues with finding the target directory for command files during global installation.

## 5.2.6

### Patch Changes

- Use the proxy settings not only during resolution but also when fetching tarballs.

## 5.2.5

### Patch Changes

- Fixing some regressions with global bin directory caused by v5.2.4.

## 5.2.4

### Patch Changes

- Find the proper directory for linking executables during global installation.
- Fix `pnpm list --long`. This was a regression in pnpm v5.0.0.

## 5.2.3

### Patch Changes

- Own implementation of the `pnpm bin` command added (previously it was passed through to `npm bin`).
- Read the correct PATH env variable on all systems, when running pnpx. One Windows the correct path name might be Path or other.
- Install the pnpm bin executable to the directory of the globally installed pnpm executable, when running `pnpm add -g pnpm`.
- `pnpm store prune` should not fail when the store has some foreign files.
- `pnpm unlink --global` should unlink bin files from the global executables directory.

## 5.2.2

### Patch Changes

- Don't remove skipped optional dependencies from the current lockfile on partial installation.

## 5.2.1

### Patch Changes

- Hoisting should not fail if some of the aliases cannot be hoisted due to issues with the lockfile.

## 5.2.0

### Minor Changes

- Added a new setting: `public-hoist-pattern`. This setting can be overwritten by `shamefully-hoist`. The default value of `public-hoist-pattern` is `types/*`.

  If `shamefully-hoist` is `true`, `public-hoist-pattern` is set to `*`.

  If `shamefully-hoist` is `false`, `public-hoist-pattern` is set to nothing.

  `public-hoist-pattern` example configuration (through a `.npmrc` file):

  ```
  public-hoist-pattern[]=@types/*
  public-hoist-pattern[]=@angular/*
  ```

  Related PR: #2631

- Don't request the full metadata of package when running `pnpm outdated` or `pnpm publish -r` (#2633)

## 5.1.8

### Patch Changes

- Don't fail when the installed package's manifest (`package.json`) starts with a byte order mark (BOM). This is a fix for a regression that appeared in v5.0.0 (#2629).

## 5.1.7

### Patch Changes

- When `link-workspace-packages` is `false`, filtering by dependencies/dependents should ignore any packages that are not specified via `workspace:` ranges (#2625).
- Print a "Did you mean" line under the unknown option error with any option that look similar to the typed one (#2603).

## 5.1.6

### Patch Changes

- It should be possible to install a tarball through a non-standard URL endpoint served via the registry domain.

  For instance, the configured registry is `https://registry.npm.taobao.org/`.
  It should be possible to run `pnpm add https://registry.npm.taobao.org/vue/download/vue-2.0.0.tgz`

  Related issue: #2549

## 5.1.5

### Patch Changes

- Print a warning when an HTTP request fails (#2615).
- Perform headless installation when dependencies should not be linked from the workspace, and they are not indeed linked from the workspace (#2619).

## 5.1.4

### Patch Changes

- Fix too long file name issue during write to the content-addressable store (#2605).

## 5.1.3

### Patch Changes

- Don't remove authorization headers when redirecting requests to the same host (#2602).

## 5.1.2

### Patch Changes

- fix an issue with node-gyp failure. Downgrade uuid.

## 5.1.1

### Patch Changes

- 86d21759d: Print a meaningful error when pnpm is executed with Node.js v13.0-v13.6

## 5.1.0

### Minor Changes

- ffddf34a8: Add new global option called `--stream`. (#2595)

  When used, the output from child processes is streamed to the console immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.

- The `run` and `exec` commands may use the `--parallel` option.

  `--parallel` completely disregards concurrency and topological sorting,
  running a given script immediately in all matching packages
  with prefixed streaming output. This is the preferred flag
  for long-running processes such as watch run over many packages.

  For example: `pnpm run --parallel watch`

  PR #2599

- Color the child output prefixes (#2598)

### Patch Changes

- A recursive run should not rerun the same package script which started the lifecycle event (#2528).
- Fixing a regression on Windows. Fall back to copying if linking fails (429c5a560b7a32b0261e471ece349ec136ab7f4d)

## 5.0.2

### Patch Changes

- 2f9c7ca85: Fix a regression introduced in pnpm v5.0.0.
  Create correct lockfile when the package tarball is hosted not under the registry domain.
- 160975d62: This fixes a regression introduced in pnpm v5.0.0. Direct local tarball dependencies should always be reanalized on install.

## 5.0.1

### Patch Changes

- 81b537003: The usage of deprecated options should not crash the CLI. When a deprecated option is used (like `pnpm install --no-lock`), just print a warning.
- 187615f87: Fix installation of git-hosted packages. This was a regression in v5.

## 5.0.0

### Major Changes

- 🚀 33% faster installation times vs pnpm v4.

  In some cases, 2 times faster than Yarn v1! ([performance diff of pnpm v4 vs v5](https://github.com/pnpm/benchmarks-of-javascript-package-managers/commit/5328f0165628b0ee5e22a8a433357d65bee75d64))

  | action  | cache | lockfile | node_modules | npm   | pnpm  | Yarn  | Yarn PnP |
  | ------- | ----- | -------- | ------------ | ----- | ----- | ----- | -------- |
  | install |       |          |              | 43.3s | 17.5s | 36.7s | 28.6s    |
  | install | ✔     | ✔        | ✔            | 7s    | 1.5s  | 735ms | n/a      |
  | install | ✔     | ✔        |              | 18.3s | 7.8s  | 10.5s | 1.8s     |
  | install | ✔     |          |              | 24.8s | 10.9s | 22.2s | 12.1s    |
  | install |       | ✔        |              | 23.2s | 15.2s | 22.4s | 13.4s    |
  | install | ✔     |          | ✔            | 6.4s  | 1.8s  | 17.1s | n/a      |
  | install |       | ✔        | ✔            | 7.3s  | 1.5s  | 735ms | n/a      |
  | install |       |          | ✔            | 6.4s  | 3.1s  | 33.2s | n/a      |
  | update  | n/a   | n/a      | n/a          | 7s    | 14.5s | 42.6s | 27.6s    |

  All the benchmarks are [here](https://github.com/pnpm/benchmarks-of-javascript-package-managers/tree/5328f0165628b0ee5e22a8a433357d65bee75d64).

- A content-addressable filesystem is used to store packages on the disk.

  pnpm v5 uses a content-addressable filesystem to store all files from all module directories on a disk. If you depend on different versions of lodash, only the files that differ are added to the store. If lodash has 100 files, and a new version has a change only in one of those files, pnpm update will only add 1 new file to the storage.

  For more info about the structure of this new store, you can check the [GitHub issue about it](https://github.com/pnpm/pnpm/issues/2470).

  This change was inspired by [dupe-krill](https://github.com/kornelski/dupe-krill) and the content-addressable storage of Git.

- Reduced directory nesting in the virtual store directory.

  In pnpm v4, if you installed `foo@1.0.0`, it was hard-linked into `node_modules/.pnpm/registry.npmjs.org/foo/1.0.0/`.

  In pnpm v5, it will be hard-linked into `node_modules/.pnpm/foo@1.0.0/`. This new structure of the virtual store directory drastically reduces the number of directories pnpm has to create. Hence, there are fewer filesystem operations, which improves speed.

- `pnpm store usages` removed.

  This command was using information from the `store.json` files, which is not present in the new content-addressable storage anymore.

- The `independent-leaves` setting has been removed.

  When hoisting was off, it was possible to set the `independent-leaves` setting to `true`. When `true`, leaf dependencies were symlinked directly from the global store. However, we turned hoisting on by default for pnpm v4, so this feature has no future at the moment.

- The `resolution-strategy` setting has been removed.

  By default, the `fewer-dependencies` resolution strategy is used. It was possible to select a `fast` resolution strategy. This setting is deprecated to simplify future improvements to the resolution algorithm.

- The store and the modules directory are not locked.

  We are not using directory locks anymore. So the `--no-lock` option will throw an error. Some users had [issues](https://github.com/pnpm/pnpm/issues/594) with locking. We have confidence that pnpm will never leave either node_modules or the store in a broken state,
  so we removed locking.

- `git-checks` is `true` by default.

  By default, `pnpm publish` will make some checks before actually publishing a new version of your package.

  The next checks will happen:

  - The current branch is your publish branch. The publish branch is `master` by default. This is configurable through the `publish-branch` setting.
  - Your working directory is clean (there are no uncommitted changes).
  - The branch is up-to-date.

  If you don't want this checks, run `pnpm publish --no-git-checks` or set this setting to `false` via a `.npmrc` file.

- In case of a crash, the debug file will be written to `node_modules/.pnpm-debug.log` (not to `pnpm-debug.log` as in v4 and earlier).

### Minor Changes

- The `link-workspace-packages` setting may now be set to `deep`.

  When `link-workspace-packages` is set to `deep`, packages from the workspace will be linked even to subdependencies.
