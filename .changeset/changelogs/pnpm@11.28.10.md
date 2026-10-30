## 11.28.10

### Patch Changes

- The Node.js `latest` selector now chooses the newest stable release [pnpm/pnpm#17490](https://github.com/pnpm/pnpm/issues/17490).

- On Windows, the shell shim of a `.cmd` or `.bat` bin now runs it under Git Bash when MSYS argument conversion is off, such as with `MSYS_NO_PATHCONV=1` or `MSYS2_ARG_CONV_EXCL='*'`. The shim used to start an interactive cmd.exe prompt instead [#17480](https://github.com/pnpm/pnpm/issues/17480).

- `pnx` and `pnpm create` now run the command in the directory selected by `--dir` or `--workspace-root`. Without either flag, the command still runs in the directory pnpm was started in.

- The standalone `@pnpm/exe` builds now embed Node.js 26.13.0.

- Preserve workspace importers named `__proto__` when merging lockfiles [pnpm/tasks#59](https://github.com/pnpm/tasks/issues/59).

- Commands installed with `pnpm add --global` now find the dependencies that pnpm hoists for their package. A package that loads a dependency it does not declare, such as a native addon's platform-specific package, failed with "Cannot find module". Reinstall such a package with `pnpm add --global` to update its existing command [#17503](https://github.com/pnpm/pnpm/issues/17503).

- With the global virtual store, a change to a dependency of one package in a dependency cycle now gives every package in that cycle a new directory. Before, the other packages kept their directories and still linked the old copy of the changed package, so the project could load two copies of it [#17466](https://github.com/pnpm/pnpm/issues/17466).

  Packages in a dependency cycle, and packages that depend on one, move to new global virtual store directories once after this update. Those that run a build miss the side-effects cache once.

- With `injectWorkspacePackages` and per-project lockfiles, `pnpm install` now updates a project's lockfile when a local tarball that an injected workspace project depends on is replaced. The project's lockfile kept the old version and integrity before.

- The error for a dependency with an invalid name in a project's manifest now names the project's directory. It used to say only "The current package", which did not tell which workspace project to fix [#17475](https://github.com/pnpm/pnpm/issues/17475).

- `pnpm install` now resolves a local directory again after its `package.json` changes when an override or a package extension adds it to the dependencies of a registry package. `pnpm install --frozen-lockfile` fails on a lockfile that still records the old dependencies of such a directory [#17486](https://github.com/pnpm/pnpm/issues/17486).

- `pnpm pack` and `pnpm publish` now always include the binary file declared by a string `bin` field in `package.json`, even when `files` or `.npmignore` would otherwise exclude it.

- `pnpm store prune` preserves other stores and global package data when `cacheDir` points to their parent directory.

- `pnpm prune --prod` no longer fails with `ERR_PNPM_EXDEV` when `node_modules` was copied from an earlier Docker build stage [#17424](https://github.com/pnpm/pnpm/issues/17424).

- `pnpm add` no longer rewrites a catalog entry set to the dist-tag that the dependency resolves through. For example, an entry `beta` stays `beta` after `pnpm add foo@beta` or `pnpm add foo`. Before, the entry was replaced with the version the tag pointed to [pnpm/tasks#225](https://github.com/pnpm/tasks/issues/225).

- `pnpm install` and `pnpm rebuild` now refresh privately hoisted `.bin` shims in `node_modules/.pnpm/node_modules/.bin` after a dependency build replaces a script target with a native executable [#17402](https://github.com/pnpm/pnpm/issues/17402).

- `pnpm install` now relinks a `workspace:` or `link:` dependency whose link was deleted from `node_modules`. It used to report "Already up to date" and leave the link missing [#17485](https://github.com/pnpm/pnpm/issues/17485).

- `pnpm install` now detects a change to a local tarball or directory that only a pnpmfile `readPackage` hook depends on [pnpm/tasks#227](https://github.com/pnpm/tasks/issues/227). A repeat install reported "Already up to date" and kept the old contents before.

- `pnpm install` now retries a Node.js runtime zip download when the connection drops partway through. A download that still fails names its URL. Before, a single dropped connection ended the install with `terminated` [#17411](https://github.com/pnpm/pnpm/issues/17411).

- Commands, `pnpm --version`, `pnpm self-update`, and invocations with unknown flags now apply the same `packageManager` and `devEngines.packageManager` settings when switching to the pinned pnpm version or recording it in `pnpm-lock.yaml`. `--config.<key>` flags and `lockfileDir` now reach the version switch and lockfile sync on every entry point. Unreadable settings in `pnpm-workspace.yaml` or the global `config.yaml` that do not affect the pin decision no longer block the version switch [pnpm/tasks#222](https://github.com/pnpm/tasks/issues/222).

- When the version or range given to `pnpm unpublish` matches every published version, the error now says so and shows the same command with `--force`. It suggested running `pnpm unpublish <pkg>@<version>`, the command that had just failed.
