# @pnpm/config

## 15.1.4

### Patch Changes

- ae2f845c5: `NODE_ENV=production pnpm install --dev` should only install dev deps [#4745](https://github.com/pnpm/pnpm/pull/4745).

## 15.1.3

### Patch Changes

- 05159665d: Do not return a default value for the node-version setting.

## 15.1.2

### Patch Changes

- af22c6c4f: When the global bin directory is set to a symlink, check not only the symlink in the PATH but also the target of the symlink [#4744](https://github.com/pnpm/pnpm/issues/4744).

## 15.1.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/pnpmfile@2.0.2
  - @pnpm/read-project-manifest@3.0.2

## 15.1.0

### Minor Changes

- e05dcc48a: New setting added to turn back v6 directory filtering that doesn't require globs: `legacy-dir-filtering`.

## 15.0.0

### Major Changes

- 546e644e9: Don't hoist types by default to the root of `node_modules` [#4459](https://github.com/pnpm/pnpm/pull/4459).
- 4bed585e2: The next deprecated settings were removed:

  - frozen-shrinkwrap
  - prefer-frozen-shrinkwrap
  - shared-workspace-shrinkwrap
  - shrinkwrap-directory
  - lockfile-directory
  - shrinkwrap-only
  - store

### Minor Changes

- 8dac029ef: Any package with "prettier" in its name is hoisted.
- c6463b9fd: New setting added: `git-shallow-hosts`. When cloning repositories from "shallow-hosts", pnpm will use shallow cloning to fetch only the needed commit, not all the history [#4548](https://github.com/pnpm/pnpm/pull/4548).
- 8fa95fd86: The default value of `nodeLinker` is set to `isolated`.

### Patch Changes

- 72b79f55a: Setting the `auto-install-peers` to `true` should work.
- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0
  - @pnpm/error@3.0.1
  - @pnpm/pnpmfile@2.0.1
  - @pnpm/read-project-manifest@3.0.1

## 14.0.0

### Major Changes

- 516859178: `extendNodePath` removed.
- 73d71a2d5: `strict-peer-dependencies` is `true` by default.
- fa656992c: The `embed-readme` setting is `false` by default.
- 542014839: Node.js 12 is not supported.
- 585e9ca9e: `pnpm install -g pkg` will add the global command only to a predefined location. pnpm will not try to add a bin to the global Node.js or npm folder. To set the global bin directory, either set the `PNPM_HOME` env variable or the [`global-bin-dir`](https://pnpm.io/npmrc#global-bin-dir) setting.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/error@3.0.0
  - @pnpm/pnpmfile@2.0.0
  - @pnpm/read-project-manifest@3.0.0

## 13.13.2

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/global-bin-dir@3.0.1
  - @pnpm/pnpmfile@1.2.6
  - @pnpm/read-project-manifest@2.0.13

## 13.13.1

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/pnpmfile@1.2.5
  - @pnpm/read-project-manifest@2.0.12

## 13.13.0

### Minor Changes

- 334e5340a: Add support of the `update-notifier` configuration option [#4158](https://github.com/pnpm/pnpm/issues/4158).

## 13.12.0

### Minor Changes

- b7566b979: embed-readme option was added

## 13.11.0

### Minor Changes

- fff0e4493: Set `side-effects-cache-read` and `side-effects-cache-write`.

## 13.10.0

### Minor Changes

- e76151f66: New setting supported: `auto-install-peers`. When it is set to `true`, `pnpm add <pkg>` automatically installs any missing peer dependencies as `devDependencies`.

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/pnpmfile@1.2.4
  - @pnpm/read-project-manifest@2.0.11

## 13.9.0

### Minor Changes

- 8fe8f5e55: New CLI option: `--ignore-workspace`. When used, pnpm ignores any workspace configuration found in the current or parent directories.

## 13.8.0

### Minor Changes

- 732d4962f: nodeLinker may accept two new values: `isolated` and `hoisted`.

  `hoisted` will create a "classic" `node_modules` folder without using symlinks.

  `isolated` will be the default value that creates a symlinked `node_modules`.

- a6cf11cb7: `userConfig` added to the config object, which contain only the settings set in the user's home config file.

## 13.7.2

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/pnpmfile@1.2.3
  - @pnpm/read-project-manifest@2.0.10

## 13.7.1

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/pnpmfile@1.2.2
  - @pnpm/read-project-manifest@2.0.9

## 13.7.0

### Minor Changes

- 927c4a089: A new option `--aggregate-output` for `append-only` reporter is added. It aggregates lifecycle logs output for each command that is run in parallel, and only prints command logs when command is finished.

  Related discussion: [#4070](https://github.com/pnpm/pnpm/discussions/4070).

- 10a4bd4db: New option added for: `node-mirror:<releaseDir>`. The string value of this dynamic option is used as the base URL for downloading node when `use-node-version` is specified. The `<releaseDir>` portion of this argument can be any dir in `https://nodejs.org/download`. Which `<releaseDir>` dynamic config option gets selected depends on the value of `use-node-version`. If 'use-node-version' is a simple `x.x.x` version string, `<releaseDir>` becomes `release` and `node-mirror:release` is read. Defaults to `https://nodejs.org/download/<releaseDir>/`.

### Patch Changes

- 30bfca967: When normalizing registry URLs, a trailing slash should only be added if the registry URL has no path.

  So `https://registry.npmjs.org` is changed to `https://registry.npmjs.org/` but `https://npm.pkg.github.com/owner` is unchanged.

  Related issue: [#4034](https://github.com/pnpm/pnpm/issues/4034).

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/pnpmfile@1.2.1
  - @pnpm/read-project-manifest@2.0.8

## 13.6.1

### Patch Changes

- 46aaf7108: Revert the change that was made in pnpm v6.23.2 causing a regression describe in [#4052](https://github.com/pnpm/pnpm/issues/4052).

## 13.6.0

### Minor Changes

- 8a99a01ff: Read the root project manifest and write it to the config object.

## 13.5.1

### Patch Changes

- a7ff2d5ce: When normalizing registry URLs, a trailing slash should only be added if the registry URL has no path.

  So `https://registry.npmjs.org` is changed to `https://registry.npmjs.org/` but `https://npm.pkg.github.com/owner` is unchanged.

  Related issue: [#4034](https://github.com/pnpm/pnpm/issues/4034).

## 13.5.0

### Minor Changes

- 002778559: New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

## 13.4.2

### Patch Changes

- Updated dependencies [302ae4f6f]
- Updated dependencies [b75993dde]
  - @pnpm/pnpmfile@1.2.0
  - @pnpm/types@7.6.0

## 13.4.1

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/pnpmfile@1.1.1

## 13.4.0

### Minor Changes

- b6d74c545: Allow a system's package manager to override pnpm's default settings

## 13.3.0

### Minor Changes

- bd7bcdbe8: New setting supported: maxsockets. maxsockets allows to set the maximum number of connections to use per origin (protocol/host/post combination).

## 13.2.0

### Minor Changes

- 5ee3b2dc7: New setting: `configDir`.

## 13.1.0

### Minor Changes

- 4027a3c69: New optional field added to the Config object: hooks.

### Patch Changes

- Updated dependencies [ef9d2719a]
- Updated dependencies [4027a3c69]
  - @pnpm/pnpmfile@1.1.0

## 13.0.0

### Major Changes

- c7081cbb4: NODE_PATH is not set in the command shims of globally installed packages.

### Minor Changes

- fe5688dc0: Add option 'changed-files-ignore-pattern' to ignore changed files by glob patterns when filtering for changed projects since the specified commit/branch.
- c7081cbb4: New option added: `extendNodePath`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims.

## 12.6.0

### Minor Changes

- d62259d67: Use a subfolder of the pnpm homedir as the location of globally installed packages, when use-beta-cli is on.

## 12.5.0

### Minor Changes

- 6681fdcbc: New setting added: `global-bin-dir`. `global-bin-dir` allows to set the target directory for the bin files of globally installed packages.

## 12.4.9

### Patch Changes

- ede519190: Fix a bug that doesn't respect `cache-dir`/`state-dir` paths from configuration files

## 12.4.8

### Patch Changes

- Updated dependencies [47a1e9696]
  - @pnpm/global-bin-dir@3.0.0

## 12.4.7

### Patch Changes

- 655af55ba: The default home directory for pnpm on macOS should be at `~/Library/pnpm`.

## 12.4.6

### Patch Changes

- 3fb74c618: Don't ignore the `--workspace-root` option.

## 12.4.5

### Patch Changes

- 051296a16: workspaceRoot should only be read for CLI options.

## 12.4.4

### Patch Changes

- af8b5716e: pnpm should always have write access to its home directory

## 12.4.3

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0

## 12.4.2

### Patch Changes

- 73c1f802e: Choose the right location for global dir.

## 12.4.1

### Patch Changes

- 2264bfdf4: Choose proper default state-dir and cache-dir on macOS.

## 12.4.0

### Minor Changes

- 25f6968d4: Add `workspace-concurrency` based on CPU cores amount, just set `workspace-concurrency` as zero or negative, the concurrency limit is set as `max((amount of cores) - abs(workspace-concurrency), 1)`
- 5aaf3e3fa: New setting added: stateDir.

## 12.3.3

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0

## 12.3.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0

## 12.3.1

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 12.3.0

### Minor Changes

- 84ec82e05: New setting added: `use-node-version`. When set, pnpm will install the specified version of Node.js and use it for running any lifecycle scripts.
- c2a71e4fd: New CLI option added: `use-stderr`. When set, all the output is written to stderr.
- 84ec82e05: New settings are returned: pnpmExecPath and pnpmHomeDir.

## 12.2.0

### Minor Changes

- 05baaa6e7: Add new config setting: `fetch-timeout`.
- dfdf669e6: Add new cli arg --filter-prod. --filter-prod acts the same as --filter, but it omits devDependencies when building dependencies

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0

## 12.1.0

### Minor Changes

- ba5231ccf: New option added for: `enable-pre-post-scripts`. When it is set to `true`, lifecycle scripts with pre/post prefixes are automatically executed by pnpm.

## 12.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- aed712455: Remove `pnpm-prefix` setting support.
- aed712455: `globalDir` is never set. Only the `dir` option is set with the global directory location when the `--global` is used. The pnpm CLI should have access to the global dir, otherwise an exception is thrown.

### Minor Changes

- 78470a32d: New setting added: `modules-cache-max-age`. The default value of the setting is 10080 (7 days in seconds). `modules-cache-max-age` is the time in minutes after which pnpm should remove the orphan packages from node_modules.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - @pnpm/error@2.0.0
  - @pnpm/global-bin-dir@2.0.0
  - @pnpm/types@7.0.0

## 11.14.2

### Patch Changes

- 4f1ce907a: Add type for `noproxy`.

## 11.14.1

### Patch Changes

- 4b3852c39: The noproxy setting should work.

## 11.14.0

### Minor Changes

- cb040ae18: add option to check unknown settings

## 11.13.0

### Minor Changes

- c4cc62506: Add '--reverse' flag for reversing the order of package executions during 'recursive run'

## 11.12.1

### Patch Changes

- bff84dbca: fix: remove empty keys from config key check

## 11.12.0

### Minor Changes

- 548f28df9: print warnings if unknown settings are found in .npmrc

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0

## 11.11.1

### Patch Changes

- Updated dependencies [941c5e8de]
  - @pnpm/global-bin-dir@1.2.6

## 11.11.0

### Minor Changes

- f40bc5927: New option added: enableModulesDir. When `false`, pnpm will not write any files to the modules directory. This is useful for when you want to mount the modules directory with FUSE.

## 11.10.2

### Patch Changes

- 425c7547d: Always resolve the target directory to its real path.

## 11.10.1

### Patch Changes

- ea09da716: The test-pattern option should be an Array.

## 11.10.0

### Minor Changes

- a8656b42f: New option added: `test-pattern`. `test-pattern` allows to detect whether the modified files are related to tests. If they are, the dependent packages of such modified packages are not included.

## 11.9.1

### Patch Changes

- 041537bc3: Finding global bin directory on Windows.

## 11.9.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

## 11.8.0

### Minor Changes

- fcc1c7100: Add prettier plugins to the default public-hoist-pattern list

## 11.7.2

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/global-bin-dir@1.2.5

## 11.7.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1

## 11.7.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

## 11.6.1

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0

## 11.6.0

### Minor Changes

- f591fdeeb: New option added: `node-linker`. When `node-linker` is set to `pnp`, pnpm will create a `.pnp.js` file.

## 11.5.0

### Minor Changes

- 74914c178: New experimental option added for installing node_modules w/o symlinks.

## 11.4.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

### Patch Changes

- Updated dependencies [846887de3]
  - @pnpm/global-bin-dir@1.2.4

## 11.3.0

### Minor Changes

- 092f8dd83: New setting added: workspace-root.

### Patch Changes

- 767212f4e: Packages like @babel/types should be publicly hoisted by default.

## 11.2.7

### Patch Changes

- 9f1a29ff9: During global install, changes should always be saved to the global package.json, even when save is set to false.
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/global-bin-dir@1.2.3

## 11.2.6

### Patch Changes

- ac0d3e122: Publicly hoist any dependency that is related to ESLint.

## 11.2.5

### Patch Changes

- 972864e0d: When public-hoist-pattern is set to an empty string or a list with a single empty string, then it is considered to be undefined.
- Updated dependencies [4d4d22b63]
  - @pnpm/global-bin-dir@1.2.2

## 11.2.4

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/global-bin-dir@1.2.1

## 11.2.3

### Patch Changes

- 13c18e397: Stop searching for local prefix, when directory has a `package.json5` or `package.yaml`.

## 11.2.2

### Patch Changes

- 3f6d35997: Don't read the `.npmrc` files that are outside of the current workspace.

## 11.2.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 11.2.0

### Minor Changes

- ad69677a7: A new option added that allows to resolve the global bin directory from directories to which there is no write access.

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/global-bin-dir@1.2.0

## 11.1.0

### Minor Changes

- 65b4d07ca: feat: add config to make install only install package dependencies in a workspace
- ab3b8f51d: Hoist all ESLint plugins to the root of node_modules by default.

## 11.0.1

### Patch Changes

- Updated dependencies [245221baa]
  - @pnpm/global-bin-dir@1.1.1

## 11.0.0

### Major Changes

- 71aeb9a38: Remove proxy from the object returned by @pnpm/config. httpsProxy and httpProxy are returned instead.

### Minor Changes

- 915828b46: A new setting is returned by `@pnpm/config`: `npmGlobalBinDir`.
  `npmGlobalBinDir` is the global executable directory used by npm.

  This new config is used by `@pnpm/global-bin-dir` to find a suitable
  directory for the binstubs installed by pnpm globally.

### Patch Changes

- Updated dependencies [915828b46]
  - @pnpm/global-bin-dir@1.1.0

## 10.0.1

### Patch Changes

- Updated dependencies [2c190d49d]
  - @pnpm/global-bin-dir@1.0.1

## 10.0.0

### Major Changes

- db17f6f7b: Move Project and ProjectsGraph to @pnpm/types.
- 1146b76d2: `globalBin` is removed from the returned object.

  The value of `bin` is set by the `@pnpm/global-bin-dir` package when the `--global` option is used.

### Patch Changes

- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/global-bin-dir@1.0.0
  - @pnpm/types@6.2.0

## 9.2.0

### Minor Changes

- 71a8c8ce3: Added a new setting: `public-hoist-pattern`. This setting can be overwritten by `--[no-]shamefully-hoist`. The default value of `public-hoist-pattern` is `types/*`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 9.1.0

### Minor Changes

- ffddf34a8: Add new global option called `--stream`.
  When used, the output from child processes is streamed to the console immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.

## 9.0.0

### Major Changes

- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [da091c711]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/types@6.0.0
  - @pnpm/error@1.2.1

## 9.0.0-alpha.2

### Major Changes

- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- Updated dependencies [ca9f50844]
  - @pnpm/constants@4.0.0-alpha.1

## 8.3.1-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0

## 8.3.1-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
