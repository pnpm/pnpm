## 12.16.1

### Patch Changes

- pnpm now accepts the `--cache-dir <dir>` flag, as in `pnpm install --cache-dir /tmp/pnpm-cache` [#17422](https://github.com/pnpm/pnpm/issues/17422).

- `pnpm install` no longer hangs on a cold store in a workspace that also installs Cargo crates or a Rust toolchain.

- `pnpm install` now relinks a command that a workspace project renames into the projects that depend on it when `sharedWorkspaceLockfile` is `false` [pnpm/tasks#224](https://github.com/pnpm/tasks/issues/224).

- `pnpm pack` and `pnpm publish` now apply a `files` entry that names a file after the glob and directory entries, as npm does. A file excluded by name, such as `!dist/index.js`, stays out even when a later `dist/**` matches it. A file named by its path, such as `lib/secret.js`, ships even when a later `!lib/*.js` matches it.

- `pnpm install` now materializes a symlink to an unpacked file in an injected workspace dependency as a regular file when hardlinking. Git dependency fetches now skip internal directory symlinks when importing files into the store.

- Peer dependencies are recalculated from package manifests when an install updates an existing lockfile.

- `pnpm install` and `pnpm rebuild` now refresh privately hoisted `.bin` shims in `node_modules/.pnpm/node_modules/.bin` after a dependency build replaces a script target with a native executable [#17402](https://github.com/pnpm/pnpm/issues/17402).

- Commands, `pnpm --version`, `pnpm self-update`, and invocations with unknown flags now apply the same `packageManager` and `devEngines.packageManager` settings when switching to the pinned pnpm version or recording it in `pnpm-lock.yaml`. `--config.<key>` flags and `lockfileDir` now reach the version switch and lockfile sync on every entry point. Unreadable settings in `pnpm-workspace.yaml` or the global `config.yaml` that do not affect the pin decision no longer block the version switch [pnpm/tasks#222](https://github.com/pnpm/tasks/issues/222).

- When the version or range given to `pnpm unpublish` matches every published version, the error now says so and shows the same command with `--force`. It suggested running `pnpm unpublish <pkg>@<version>`, the command that had just failed.
