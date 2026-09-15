## 12.4.2

pnpm 12.4.2 includes security fixes for executable shims and GitHub Actions links, more reliable installs, faster peer dependency checks in workspaces, and Python lockfiles that work across compatible targets.

### Patch Changes

#### Security

- Dependency executables can no longer take over another package's POSIX bin shim through its shell helpers. Reinstall dependencies to replace existing shims [#14837](https://github.com/pnpm/pnpm/issues/14837).

  On Cygwin, MSYS2, and WSL, shims still use `PATH` for Windows path conversion, so dependency executables can still redirect them there.

- GitHub Actions homepage links no longer expose server credentials. GitHub server URLs now require HTTPS, with HTTP allowed only for loopback hosts.

#### Installing packages

- pnpm no longer crashes at startup on FreeBSD and other Unix-like platforms. Platforms other than Windows and macOS use `~/.local/share/pnpm/store` by default [#14859](https://github.com/pnpm/pnpm/issues/14859).

- `pnpm install` on Windows no longer fails with `ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` when clearing `node_modules` containing linked dependencies, such as when changing `nodeLinker` [#14790](https://github.com/pnpm/pnpm/issues/14790).

- `pnpm install <pkg>` now accepts `--prod` and `--dev`, including `--prod=false` [#14868](https://github.com/pnpm/pnpm/issues/14868).

- `pnpm install` and `pnpm update` now honor `--ignore-workspace` in nested projects excluded from the surrounding workspace. The flag also skips that workspace's settings during the `packageManager` check [#14809](https://github.com/pnpm/pnpm/issues/14809).

- `pnpm install` on macOS no longer reuses stale files for `file:` tarball or git-hosted tarball dependencies.

- `pnpm install` in a single-project directory now detects `package.json` edits made while the previous install was finishing [#14890](https://github.com/pnpm/pnpm/issues/14890).

- `pnpm install --frozen-lockfile` now removes packages no longer reachable from any project in `pnpm-lock.yaml`. This also prevents repeated lifecycle script execution and unnecessary installs before `pnpm run` and `pnpm exec` with `verifyDepsBeforeRun` [#14891](https://github.com/pnpm/pnpm/issues/14891).

#### Resolving and updating dependencies

- Node.js runtime resolution now reports network failures from unofficial-builds.nodejs.org. These failures previously omitted musl builds from `pnpm-lock.yaml`, making its contents depend on network access [#14813](https://github.com/pnpm/pnpm/issues/14813).

- `pnpm install` now rejects invalid `peerDependencies` specifiers with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION`. A value such as `"foo": "foo@1.0.0"` previously created a broken directory link [#14791](https://github.com/pnpm/pnpm/issues/14791).

- `pnpm deploy` now writes plain registry versions in the deployed `package.json`, without peer dependency suffixes. The lockfile retains peer bindings, and npm aliases retain their target package names [#14873](https://github.com/pnpm/pnpm/issues/14873).

- `pnpm add <git repository>` now names repositories without a `package.json` as `@owner/repo`, allowing dependencies on equally named repositories from different owners [#14870](https://github.com/pnpm/pnpm/issues/14870).

- Peer dependency resolution now deduplicates packages whose child dependency resolves an optional peer in only some workspace projects, such as `next` with `styled-jsx`'s optional `babel-plugin-macros` peer [#14800](https://github.com/pnpm/pnpm/issues/14800).

- `pnpm update` now settles the lockfile in one run when an upgrade removes the package providing an optional peer dependency [#14895](https://github.com/pnpm/pnpm/issues/14895).

- `pnpm update --no-save` now preserves override-applied specifiers for dependencies it is not updating, preventing subsequent frozen installs from failing with `ERR_PNPM_OUTDATED_LOCKFILE` [#14836](https://github.com/pnpm/pnpm/issues/14836).

- `pnpm update --no-save` now succeeds under `minimumReleaseAgeStrict` when every resolved version is old enough [#14835](https://github.com/pnpm/pnpm/issues/14835).

#### Performance

- Workspace installs and `pnpm peers check` are faster when projects depend on each other, fixing a slowdown introduced in 12.3.0. Unmet peer dependencies of workspace packages are now reported only under projects that link them directly [#14906](https://github.com/pnpm/pnpm/issues/14906).

- Hoisted installs use less memory when packages are cached. Frozen-lockfile hoisted installs on macOS are also faster when reusable package directories are cached.

#### Python projects

- `pnpm install --frozen-lockfile` now reuses `pylock.toml` across compatible Python targets, including after kernel updates. Reuse requires unchanged requirements, index, and `requires-python`, compatible wheels, and a locked dependency graph matching the target's markers [#14843](https://github.com/pnpm/pnpm/issues/14843).

  The lockfile's `environments` marker now includes only the interpreter version and marker variables used by the dependency graph. Without `--frozen-lockfile`, pnpm warns and resolves again when the locked graph no longer matches the target.

- Python resolution no longer fails on malformed `Requires-Python` values, such as the trailing comma in `openpyxl` 3.0.x. pnpm treats these releases as declaring no interpreter range [#14910](https://github.com/pnpm/pnpm/issues/14910).

- `pnpm add pypi:...` now rejects unsupported `--save-prefix` values before editing the manifest or resolving dependencies.

#### Workspaces and scripts

- Scripts listed in `syncInjectedDepsAfterScripts` no longer fail with `ERR_PNPM_INJECTED_DEPS_SYNC_READ_DIR` when the lockfile contains an injected package copy that no project depends on.

- `shellEmulator` now expands `${VAR}`, `${VAR:-default}`, and `${VAR:+alternative}` in scripts [#14814](https://github.com/pnpm/pnpm/issues/14814).

- Cargo and Python project discovery now honors `!` exclusions in `pnpm-workspace.yaml` `packages`, skipping both parsing and generated source configuration for excluded projects [#14844](https://github.com/pnpm/pnpm/issues/14844).

- `pnpm --filter "./packages/{app,lib}"` now selects either alternative. Brace alternatives can nest, span path separators, and combine with other wildcards.

- GitHub Actions updates now stop if an action reference changes during version resolution, and preserve unrelated workflow edits.

#### CLI and output

- `pn`, `pnpx`, and `pnx` now run the pnpm installed alongside them, even when that directory is absent from `PATH` or another pnpm comes first [#14803](https://github.com/pnpm/pnpm/issues/14803).

- `pnpm --version` now reports failures to install or record a project's pinned pnpm, then prints the running CLI's version. It also honors `--store-dir` and `--store` [#14831](https://github.com/pnpm/pnpm/issues/14831).

- `pnpm self-update` no longer reinstalls the active version when it was installed by the standalone installation script [#14823](https://github.com/pnpm/pnpm/issues/14823).

- `pnpm t` and `pnpm tst` work again as aliases for `pnpm test`.

- `pnpm sbom` now emits valid repository URLs in CycloneDX `externalReferences[].url` and SPDX `homepage`. Shorthands such as `vercel/ms` become `git+https` URLs, embedded credentials are removed, and invalid repository values are omitted [#14773](https://github.com/pnpm/pnpm/issues/14773).
