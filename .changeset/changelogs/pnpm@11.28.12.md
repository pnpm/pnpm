## 11.28.12

pnpm 11.28.12 fixes many peer dependency, `pnpm deploy`, frozen-lockfile, and `nodeLinker: hoisted` bugs. It also includes a security fix. A `${VAR}` placeholder in a repository's registry URL now fails the command, so pnpm no longer sends those requests to another registry. The bundled `node-gyp` no longer ships the undici version that security scanners flag, and remote resolution needs pnpr 0.1.0-alpha.20 or newer.

### Patch Changes

#### Upgrading and security

- A `${VAR}` placeholder in a registry URL in `pnpm-workspace.yaml` or the project `.npmrc` now fails the command. pnpm does not expand environment variables in these files. Before, pnpm ignored such a URL and resolved packages from another registry, such as npmjs.org. pnpm now warns when it ignores a placeholder in a proxy, `pnprServer`, or `userAgent` setting in `pnpm-workspace.yaml` [#17762](https://github.com/pnpm/pnpm/issues/17762).

- The `node-gyp` bundled with pnpm no longer ships undici 6, which security scanners flag for CVE-2026-84947. On Node.js 26, `node-gyp` downloads through a proxy or with a custom CA file no longer crash with "invalid onError method" [#17941](https://github.com/pnpm/pnpm/issues/17941).

- Remote resolution and shared build caches no longer work with pnpr 0.1.0-alpha.19 or older. pnpm now reads the capabilities a pnpr server advertises under `pnpr.protocols`. Installs fail when the server does not confirm the requested resolution and verification policies. Upgrade the pnpr server before upgrading pnpm.

#### Installs that failed

- `pnpm install --frozen-lockfile` now succeeds when a workspace project recorded in `pnpm-lock.yaml` has an empty directory, such as a git submodule that was not checked out [#17712](https://github.com/pnpm/pnpm/issues/17712).

- `pnpm install --frozen-lockfile` now accepts a lockfile when an injected workspace package declares a `file:` or `link:` dependency with backslashes, such as `file:..\vendor`, or with a slash after the protocol, such as `file:/./vendor`. Before, the install reported the lockfile as out of date right after `pnpm install` had written it [#17720](https://github.com/pnpm/pnpm/pull/17720).

- `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` now accept a `workspace:` dependency on a project whose version is not valid semver, such as `workspace:1` on a project at version `1`. Before, they reported the lockfile that `pnpm install` had just written as out of date.

- `pnpm fetch` no longer fails with `ENOENT` when `nodeLinker` is `hoisted` and the project configures `patchedDependencies` or `allowBuilds`. `pnpm fetch` also preserves an existing hoisted `node_modules` directory [#18037](https://github.com/pnpm/pnpm/issues/18037).

- Git dependencies now accept npm's `::` separator between fragment parameters, such as `github:owner/repo#v1.0.0::path:packages/foo`. Before, pnpm read the whole fragment as a branch name and failed to resolve it [#17827](https://github.com/orgs/pnpm/discussions/17827).

- pnpm no longer fails to remove a dependency whose `package.json` is malformed. It removes the dependency and keeps its bins [pnpm/tasks#295](https://github.com/pnpm/tasks/issues/295).

- Fixed `pnpm deprecate`, `pnpm undeprecate`, and `pnpm unpublish` failing with revision conflicts when a registry cache serves stale package metadata.

- When a package injected through a path, such as `file:../lib` or `../lib`, gives one of its dependencies a `catalog:` specifier, `pnpm install` now fails the way a fresh resolution does, and `pnpm install --frozen-lockfile` reports the lockfile as outdated. Before, both could accept the existing lockfile [#17722](https://github.com/pnpm/pnpm/pull/17722).

#### Peer dependencies

- Repeated installs no longer change the peer dependency providers that the previous install chose. This covers adding a dependency, removing a shared package from one workspace project, and projects that use different ranges of one dependency [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

  An unchanged install keeps a valid locked peer provider even when a direct dependency provides another compatible version. Explicit `pnpm add` and `pnpm update` requests take precedence over it. Required dependencies no longer remain marked optional when another workspace project removes a shared package.

- Workspace projects that select different versions of a peer provider no longer share one copy of the package that depends on it. This affected peers reached through an aliased dependency or a peer dependency cycle.

- New workspace projects now prefer peer versions already pinned in the lockfile. Dependencies that are also optional peers no longer cause an unrelated project to select an older peer version.

- With `dedupePeerDependents: false`, a missing optional peer is no longer resolved from a version that only another workspace project depends on. The workspace root's dependencies still count if `resolvePeersFromWorkspaceRoot` is `true`. Before, a tool in one project's `devDependencies`, such as `webpack-cli`, could become an optional peer of another project's production dependencies [#16666](https://github.com/orgs/pnpm/discussions/16666).

- `pnpm install` no longer gives a project an optional peer dependency that it skipped because a required peer would conflict with the project's dependencies. Before, `dedupePeerDependents` merged the project's copy of the package that declares the peer with another project's copy that resolves it [#17781](https://github.com/pnpm/pnpm/issues/17781).

- `pnpm install` no longer fills in a missing optional peer dependency if a required peer it brings in conflicts with a version the project depends on. Before, such a peer was installed along with it and reported as unmet [#17718](https://github.com/orgs/pnpm/discussions/17718).

- With `autoInstallPeersFromHighestMatch`, conflicting non-semver peer dependencies now remain missing, and identical non-semver peer specifiers are preserved. Preferred registry versions no longer replace local paths or protocol-prefixed specifiers when pnpm installs missing peers [pnpm/tasks#272](https://github.com/pnpm/tasks/issues/272).

- Fixed installation and peer dependency reporting for packages named `constructor` or `toString`. Optional peers with these names are also recognized when declared only in `peerDependenciesMeta`.

#### Resolving and linking dependencies

- A root `overrides` entry that points a package at a `workspace:` path, such as `workspace:./packages/foo`, now also applies when a registry package depends on that package [pnpm/tasks#270](https://github.com/pnpm/tasks/issues/270).

- Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).

- An optional dependency that declares no `os`, `cpu`, or `libc` field is no longer skipped because its name contains only an operating system word, such as `get-windows` on macOS. pnpm now infers its platform from the name only when the name also has a cpu or libc token, such as `-win32-x64` [#17774](https://github.com/pnpm/pnpm/issues/17774).

- `pnpm dedupe` now keeps a locked runtime version, from `devEngines.runtime` or from a dependency's `engines.runtime`, when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).

- Resolving against a registry that sends `Cache-Control: no-cache`, such as Azure Artifacts, is fast again when the lockfile already decides the version. pnpm reads the cached metadata for a pinned version, or for a range the lockfile version is proven to win, without a registry request [#17758](https://github.com/pnpm/pnpm/issues/17758).

- Dependencies declared as `workspace:` entries in catalogs now remain in the lockfile when `excludeLinksFromLockfile` is enabled. Frozen installs can restore these links after `node_modules` is removed [pnpm/tasks#260](https://github.com/pnpm/tasks/issues/260).

- `pnpm install --filter` in a sparse checkout now counts the dependencies of workspace projects that are missing from disk when it resolves peer dependencies and marks packages as optional. The lockfile it writes no longer differs from a full checkout in those entries [#17791](https://github.com/pnpm/pnpm/issues/17791).

- With `nodeLinker: hoisted`, `pnpm install --filter` in a sparse checkout now drops previously installed workspace projects that are missing from disk from the hoisted `node_modules` tree. It no longer recreates `node_modules` inside a missing project's directory. Packages that only the missing project used are also removed from the root `node_modules`.

- With `nodeLinker: hoisted` and a custom `modulesDir`, pnpm now installs a workspace project's dependencies that cannot be hoisted to the root into the project's `modulesDir`. The next install that relinks the project removes the copies that earlier installs left in its `node_modules`. A later install restores the project's `modulesDir` when it was removed. Package directories that pnpm did not install are now also moved to `.ignored` in a custom `modulesDir`.

- With `dedupeDirectDeps`, `pnpm install` now compares `link:` dependencies by the directory they point at. A project's `link:` dependency is removed when the workspace root links the same directory under a different path. It is kept when the root's `link:` dependency has the same text but points at another directory. This also applies with `nodeLinker: hoisted`.

  `pnpm install` reports each removed `link:` dependency once [pnpm/tasks#293](https://github.com/pnpm/tasks/issues/293).

- A repeat `pnpm install` with `nodeLinker: pnp` now applies dependency and configuration changes. Before, it printed "Already up to date" and left the lockfile unchanged.

- Config dependencies use the declared registry `serverType` when recording and installing packages [pnpm/tasks#236](https://github.com/pnpm/tasks/issues/236).

#### pnpm deploy

- `pnpm deploy` no longer fails when the deployed project depends on a workspace package that sets `publishConfig.directory`. pnpm installs that package from its publish directory and runs its build scripts [#17771](https://github.com/pnpm/pnpm/pull/17771).

- `pnpm deploy` now runs the build scripts of a workspace project that it installs as a `file:` dependency, without a machine-specific `allowBuilds` key. An `allowBuilds` entry that denies the project still applies [#17740](https://github.com/pnpm/pnpm/issues/17740).

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [#17888](https://github.com/pnpm/pnpm/issues/17888).

- `pnpm deploy` preserves an injected workspace package's resolved dependencies when they differ from the workspace project's own dependencies. This also applies to packages with `publishConfig.directory` [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- `pnpm deploy` now binds optional peers declared only in `peerDependenciesMeta` for linked workspace packages. The deployed manifest and lockfile also retain these peers when filtering dependency groups.

#### Builds, bins, and scripts

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Packages that an older pnpm version added to the store now run their install scripts. Before, pnpm could skip those scripts and hardlink the package's files to the store. This includes git dependencies that an older pnpm marked in a shared store as not requiring a build [pnpm/tasks#189](https://github.com/pnpm/tasks/issues/189).

- Fixed `pnpm store status` reporting approved builds of git dependencies as modified when the store contains legacy build metadata.

- `pnpm store prune` now stops if a dependency link cannot be read. This keeps installed packages from being removed after an incomplete scan.

- When `pnpm install` adds placeholder entries to `allowBuilds` in `pnpm-workspace.yaml`, it no longer copies `allowBuilds` entries from other config sources into the file.

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build on macOS with the sandbox enabled. Installing again replaces the shims already in `node_modules` [#17750](https://github.com/pnpm/pnpm/issues/17750).

- Installing no longer replaces the POSIX bin shims that a newer pnpm wrote in `node_modules/.bin`.

- Linking bins is faster for packages that ship large native executables as bins. pnpm now reads only the first 512 bytes of a bin file to find its shebang [#17361](https://github.com/pnpm/pnpm/issues/17361).

- `pnpm run` and `pnpm setup` no longer clear the visible terminal on Windows when a project path contains non-ASCII characters.

- pnpm stops scripts and their descendants when it is killed during script startup [pnpm/tasks#313](https://github.com/pnpm/tasks/issues/313).

#### Engines and runtimes

- `pnpm install` now fails on a workspace project's incompatible `engines.pnpm`, or on its incompatible `engines.node` with `engineStrict` enabled, even when the project's `os`, `cpu`, or `libc` excludes the current platform [pnpm/tasks#281](https://github.com/pnpm/tasks/issues/281).

- `pnpm install` now checks the project's `engines` field when the project is already up to date. A repeat install of a project outside a workspace warns about an incompatible Node.js version, or fails with `engineStrict` enabled [#17751](https://github.com/pnpm/pnpm/issues/17751).

- A workspace project that downloads its own Node.js through `devEngines.runtime` now has its `engines.node` checked against that Node.js. Before, pnpm checked it against the shared Node.js version, so `engineStrict` could reject a project whose scripts run on a compatible runtime [pnpm/tasks#300](https://github.com/pnpm/tasks/issues/300).

- A repeat `pnpm install` now runs a full install when the `runtimeOnFail` setting changes. Before, it printed "Already up to date" and did not install the runtime a project declares in `devEngines.runtime` [pnpm/tasks#303](https://github.com/pnpm/tasks/issues/303).

- Fixed missing warnings when a required dependency does not support the current Node.js version.

#### Other commands and settings

- `pnpm dist-tag` and `pnpm unpublish` now offer browser-based authentication when the account's two-factor authentication uses a security key. Before, they asked for a one-time password in that case [#17832](https://github.com/pnpm/pnpm/issues/17832).

- `pnpm login` now prints the registry's `npm-notice` message before the web login URL. Some registries use it to show a code that you type in the browser.

- `pnpm self-update` now updates the project's `engines.pnpm` range along with its pnpm pin, keeping the `^` or `~` operator [#17732](https://github.com/pnpm/pnpm/issues/17732).

- `pnpm setup` and the install script now support PowerShell as the login shell on Linux and macOS. They add `PNPM_HOME` to `PATH` in `profile.ps1` under `$XDG_CONFIG_HOME/powershell` or `~/.config/powershell` [#18026](https://github.com/pnpm/pnpm/issues/18026).

- `pnpm setup` rejects a `PNPM_HOME` value that would split the saved `PATH` [pnpm/tasks#315](https://github.com/pnpm/tasks/issues/315).

- Relative directory selectors set on `config.filter` or `config.filterProd` by an `updateConfig` hook now resolve from the workspace root even when `--dir` is passed or `pnpm` runs from a subdirectory [#17906](https://github.com/pnpm/pnpm/issues/17906).

- pnpm now reads the boot document of `pnpm-lock.yaml` when it is written in lockfile format 13. pnpm then switches to the pnpm version such a project pins.

- Programmatic install mutations that omit workspace project manifests now resolve locally when a pnpr server is configured [pnpm/tasks#59](https://github.com/pnpm/tasks/issues/59).

- pnpm now recognizes Capawesome Cloud and RWX as CI environments.

- The warning about unrecognized settings now marks `projectScopedOptionalPeers` as a setting of a newer pnpm version.
