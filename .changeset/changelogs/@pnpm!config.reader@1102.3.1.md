## 1102.3.1

### Patch Changes

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- `pnpm env remove --global` deletes Node.js versions that pnpm installed into its own store, including when another tool installed pnpm [pnpm/pnpm#8357](https://github.com/pnpm/pnpm/issues/8357).

- On Windows, pnpm expands nested `%VAR%` references in `PNPM_HOME` and the other directory environment variables it uses for its home, store, cache, state, and config directories. pnpm fails with an error when a `%VAR%` reference remains after expansion [#13236](https://github.com/pnpm/pnpm/issues/13236).

- Throw a pnpm error when `patchedDependencies` has an invalid shape or contains a non-string value.

- `pnpm run` and `pnpm exec` no longer install dependencies automatically when the root `package.json` still keeps `overrides`, `packageExtensions`, `patchedDependencies`, or `ignoredOptionalDependencies` in its `pnpm` field. pnpm no longer reads that field, so the install rewrote the lockfile without those settings. The command now fails and asks to move the settings to `pnpm-workspace.yaml` [#16278](https://github.com/pnpm/pnpm/issues/16278).

- `pnpm root` now prints the configured `modulesDir`. It used to print `node_modules` regardless of the setting. A project's own `modulesDir` from `packageConfigs` is printed too [#9113](https://github.com/pnpm/pnpm/issues/9113).

- On Windows, if the global bin directory is not in `PATH` and a `PATH` entry still contains an unexpanded variable such as `%PNPM_HOME%`, the error now names that entry. A variable referenced from the user `Path` must be set to a full path and stored as a plain string (`REG_SZ`) for the entry to expand [pnpm/pnpm#5283](https://github.com/pnpm/pnpm/issues/5283).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.9
  - @pnpm/error@1100.2.1
  - @pnpm/hooks.pnpmfile@1100.0.34
  - @pnpm/network.git-utils@1100.0.6
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/workspace.project-manifest-reader@1100.1.1
  - @pnpm/workspace.workspace-manifest-reader@1100.2.1
