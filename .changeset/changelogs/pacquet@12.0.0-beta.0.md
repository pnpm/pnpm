## 12.0.0-beta.0

### Minor Changes

- The Rust engine now reads four more settings from `pnpm-workspace.yaml` and `PNPM_CONFIG_*`, instead of only accepting them as CLI flags:

  - `frozenLockfile` — `pnpm install` grows a `--no-frozen-lockfile` flag so the setting can be overridden in both directions. As in pnpm, it cannot be set in the global `config.yaml`.
  - `savePrefix` — the range operator `pnpm add` saves, still overridable with `--save-prefix` / `--save-exact`.
  - `savePeer` — `pnpm add` also records the new dependency in `peerDependencies`. `pnpm add --no-save-peer` overrides it back off.
  - `saveCatalogName` — the catalog `pnpm add` saves into.

- The Rust engine now supports the `saveWorkspaceProtocol` setting, so `pnpm add <pkg>@workspace:…` writes back the same specifier pnpm does. Under the default `rolling`, a request like `workspace:^1.2.3` is saved as `workspace:^` — a range with no version in it, so bumping the workspace package never has to touch its dependents' manifests. `saveWorkspaceProtocol: true` saves the workspace package's resolved version instead (`workspace:^2.5.0`), and `false` keeps the `workspace:` form only when it was asked for explicitly. Previously the specifier was written back exactly as typed.

- `pnpm update --workspace` is supported: dependencies that a workspace project publishes are re-pointed at the local copies through the `workspace:` protocol. The `saveWorkspaceProtocol` setting is honored — under its `rolling` default an entry becomes `workspace:*`, `workspace:^`, or `workspace:~` (whichever matches the range it already declared), so a sibling's next release does not invalidate it. Naming a dependency that is not in the workspace fails with `ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND`, and combining the flag with `--latest` fails with `ERR_PNPM_BAD_OPTIONS`.

  `pnpm update --depth <number>` is now applied per dependency instead of only distinguishing `0` from higher values: a dependency deeper than the given depth keeps its locked resolution, so `pnpm update --depth 0` updates direct dependencies only.

- `pnpm run "/^build:(backend|frontend)$/"` selects every script whose name matches the pattern, in single-project and recursive runs alike [#13322](https://github.com/pnpm/pnpm/issues/13322). Flags on the selector are rejected with `ERR_PNPM_UNSUPPORTED_SCRIPT_COMMAND_FORMAT`, as pnpm does.

- `pnpm self-update` no longer takes any instruction from the project it is run in:

  - pnpm is fetched through the same trusted registry and auth configuration used when switching pnpm versions, so a project `.npmrc` or `pnpm-workspace.yaml` can no longer redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is no longer loaded. Pnpmfiles from trusted sources (the `pnpmfile` setting, the global pnpmfile, config dependencies) still apply.
  - The `minimumReleaseAge` settings in `pnpm-workspace.yaml` no longer affect `self-update`. They still govern the project's own dependencies; for `self-update` the cooldown now comes from the built-in default, your global config, a `PNPM_CONFIG_*` environment variable, or a command-line flag. This fixes `self-update` failing inside a workspace that raises the cutoff while succeeding everywhere else, and stops a repository from either waiving the cooldown or keeping you on an outdated pnpm by raising it.
  - The same applies to the `trustPolicy` settings and to `ci`: a project can no longer weaken the trust check that guards the pnpm download, nor re-enable the confirmation prompt that a CI run suppresses.

  When `self-update` refuses a version that is younger than the cutoff, an interactive run now offers to update anyway; non-interactive runs still fail. CI never prompts, even on a runner that attaches a TTY.

### Patch Changes

- An aliased dependency of a protocol that resolves under its own package name — `jsr:` and the named registries — is recorded in the lockfile importer again. `"bar-from-jsr": "jsr:@pnpm-e2e/bar@1.0.0"` resolved and installed, but the importer stayed empty, so nothing reading direct dependencies out of the lockfile (`outdated`, `update`, `licenses`, dedupe, frozen-install verification) could see it [#13362](https://github.com/pnpm/pnpm/issues/13362).

- An `allowBuilds` entry with the `set this to true or false` placeholder pnpm scaffolds no longer makes every command in that workspace fail with a config-parse error [#13322](https://github.com/pnpm/pnpm/issues/13322). An undecided entry now leaves the package under the default-deny build policy, as pnpm does.

- Two `pnpm install` resolution fixes that made large workspaces such as [Astro](https://github.com/withastro/astro) produce a different `pnpm-lock.yaml` than pnpm 11 [#13334](https://github.com/pnpm/pnpm/issues/13334):

  - A scoped workspace package referenced through the `file:` protocol (`"@test/pkg": "file:./pkg"`) is recorded as a `link:` again instead of being copied in as a `file:` snapshot.
  - `bundledDependencies` / `bundleDependencies` are no longer resolved as dependencies of their own. npm ships them inside the package's tarball, so installing them again added packages the lockfile should not contain (for example `napi-wasm` under `@parcel/watcher-wasm`).

- Executables that a package ships inside its own tarball (`bundledDependencies`) are linked again into that package's `node_modules/.bin`, under both the isolated and the hoisted node linker. A package that declares `bundleDependencies: true` instead of a list of names is now recorded in `pnpm-lock.yaml` the way pnpm 11 records it, and such a lockfile can be read back.

- Aligned `pnpm licenses list --json` package metadata and license-group ordering with the TypeScript CLI.

- Fixed `pnpm --filter <package> run` to list the selected package's scripts and root workspace scripts when no script name is specified.

- Strip Unicode formatting characters from registry- and manifest-derived terminal output.

- Prevented dependency verification before scripts from rewriting an up-to-date lockfile.

- Concurrent commands in a repository that pins `packageManager` no longer race while installing the pinned pnpm version on a cold cache [#13322](https://github.com/pnpm/pnpm/issues/13322). A task runner spawning several `pnpm run` children at once could previously fail with "failed to remove existing directory … prior to swap", or leave a child looking for a binary another process had just unlinked.

- Config-load warnings, such as the warning about install settings left under the `pnpm` field of `package.json`, are printed to stderr instead of stdout [#13361](https://github.com/pnpm/pnpm/issues/13361).

- `pnpm dedupe --check` now reports what deduplication would change: the importer and package snapshot diff, the `ERR_PNPM_DEDUPE_CHECK_ISSUES` error code, and the warning that points at `pnpm peers check` when the install leaves peer-dependency issues behind. `pnpm peers check` is also accepted again — the subcommand spelling used on pnpm.io and in pnpm's own dedupe output — instead of failing with "unexpected argument 'check' found" [#13321](https://github.com/pnpm/pnpm/issues/13321).

- A deprecated package is reported once rather than once per workspace project that depends on it, and is no longer double-counted in the "deprecated subdependencies found" summary when it is also a direct dependency [#13322](https://github.com/pnpm/pnpm/issues/13322). Ignored build scripts are also listed with their `(patch_hash=…)` suffix, so two copies of a package that differ only by an applied patch are distinguishable.

- `pnpm install --frozen-lockfile` no longer re-imports a varying subset of packages on every repeat install of an unchanged project [#13316](https://github.com/pnpm/pnpm/issues/13316). The global-virtual-store directory of a package that takes part in a dependency cycle was derived from an order that changed from run to run, so those packages landed on a fresh slot each time; it is now derived deterministically and matches the directory pnpm itself computes.

- When the pinned `packageManager` engine install cannot take its lock because the store cannot be written to, pnpm now reports that instead of quietly installing without the lock. A lock another process holds is unchanged — it is still waited for.

- Speed up installs after compatible catalog or direct dependency range changes by retaining the locked version without resolving the dependency graph again.

- Speed up installs after safe override changes by reusing unambiguous compatible dependency resolutions, pruning obsolete dependencies, applying independent replacements and removals together, and handling parent-scoped `"-"` overrides without full lockfile resolution.

- Fixed warm side-effects cache reuse for git dependencies.

- Aligned `pnpm dedupe --check` progress and error output with the TypeScript CLI.

- A frozen install now fails when `autoInstallPeers`, `dedupePeers`, or `excludeLinksFromLockfile` has changed since `pnpm-lock.yaml` was written, instead of installing against a lockfile that no longer matches the settings. The error names the drifted setting, as `pnpm install --frozen-lockfile` has always done.

- A repeat `pnpm install --frozen-lockfile` is a no-op again when the project has a platform-incompatible optional dependency. The skipped package is kept in `node_modules/.pnpm/lock.yaml` (`.modules.yaml` is what records the skip), so the install can once more recognize an unchanged tree instead of re-running every lifecycle and dependency build script [#13312](https://github.com/pnpm/pnpm/issues/13312).

- Reject frozen installs when the current pnpmfile does not match the lockfile's `pnpmfileChecksum`.

- Fixed `pnpm licenses list` to report dependencies from every workspace project, exclude unsupported platform packages, and mark development dependencies.

- Setting both `autoInstallPeers: false` and `dedupePeerDependents: false` now leaves missing peers alone, instead of still installing the ones a version elsewhere in the workspace could satisfy.

- Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

  Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.

- A missing required peer is no longer auto-installed as a prerelease that its declared range rejects. A package peer-depending on `^29.0.0 || ^30.0.0` next to a `30.0.0-alpha.6` pulled in elsewhere in the graph now resolves a stable `29.x`/`30.x` from the registry instead of adopting the alpha [#13341](https://github.com/pnpm/pnpm/issues/13341).

- A lockfile entry for a git-hosted archive that records no `integrity` installs again instead of failing with `ERR_PNPM_MISSING_TARBALL_INTEGRITY`. Older pnpm versions wrote that shape for dependencies like `"ci-info": "watson/ci-info#f43f6a1c…"`, so any committed lockfile still carrying one could not be installed [#13308](https://github.com/pnpm/pnpm/issues/13308). The archive URL pins a full commit SHA, and pnpm fetches it without an integrity check.

  Every other remote tarball still has to carry an `integrity`, and the refusal now points at the repair: `pnpm clean --lockfile` followed by `pnpm install`.

  Error output no longer repeats the same message once per level of the internal error chain.

- The `Workspace` column of `pnpm update --interactive` now falls back to the project's path when its `name` is only whitespace, as it already did for a missing or empty one — all three render an equally blank label otherwise.

- `pnpm update --interactive` now groups the dependencies it offers by dependency type — `dependencies`, `devDependencies`, `optionalDependencies`, `peerDependencies`, and GitHub Actions each get their own heading — and lays each group out as a column-aligned table with a `Package`/`Current`/`Target`/`URL` header, instead of one flat list.

- `pnpm update --interactive` now measures its table in terminal columns rather than in characters. A package name, workspace name, or version containing wide characters (CJK, most emoji) no longer knocks its row's columns out of line with the rest of the group, and a wide character in a version no longer aborts the command with `Subject parameter value width cannot be greater than the container width` [#13357](https://github.com/pnpm/pnpm/issues/13357).

- `pnpm update --interactive` run inside a workspace now shows a `Workspace` column naming the project each outdated dependency was found in, so the same package outdated in several projects can be told apart.

- Write single-value `libc` package metadata in the same scalar form as pnpm.

- Fixed `pnpm install` silently skipping a local `file:*.tgz` dependency: the package is now extracted into the virtual store, recorded under `packages:` and `snapshots:`, and linked into `node_modules` [#13379](https://github.com/pnpm/pnpm/issues/13379).

- A frozen install whose recorded settings no longer match the configuration — `overrides`, `catalogs`, `patchedDependencies`, and the rest — now fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` naming the one setting that changed, instead of `ERR_PNPM_OUTDATED_LOCKFILE` with the whole map dumped [#13322](https://github.com/pnpm/pnpm/issues/13322).

- Fixed `pnpm install` dropping a package that ships no `package.json` of its own from the lockfile. Such a package is now named after its alias and recorded at version `0.0.0` under `packages:` and `snapshots:`, and its extraction gets the placeholder `package.json` pnpm writes [#13410](https://github.com/pnpm/pnpm/issues/13410).

- Resolve optional peers from versions provided by local workspace packages, omit empty deprecation messages from generated lockfiles, and preserve valid lockfile pins in `pnpm dedupe --check`.

- Aligned license reports, `dedupe --check` progress and spacing, and dependency-verification output with pnpm.

- A `file:` dependency declared by a package that was itself installed from a local directory is now resolved relative to that package's directory, not to the importer's [#13323](https://github.com/pnpm/pnpm/issues/13323). Installing a project whose local dependency depends on a sibling directory (`file:../child`) no longer fails with `Could not install from "…" as it does not exist`, and the snapshot entry for such a dependency is now written as `file:<path>` instead of `<name>@file:<path>`, matching the lockfile pnpm writes.

- `npm_config_user_agent` now carries the configured user agent (`pnpm/<version> …`) in install lifecycle scripts, `pnpm run`, `pnpm exec`, and `pnpm dlx` [#13322](https://github.com/pnpm/pnpm/issues/13322). It was previously unset for install scripts and the bare string `pnpm` elsewhere, which made `preinstall` guards that check for pnpm reject the install.

- An auto-installed *optional* peer is no longer hoisted at a version the workspace root's own dependency on that package excludes. `resolvePeersFromWorkspaceRoot` already made the workspace root's specifier decide which version a missing *required* peer is installed at; the optional-peer picker ignored it and always took the highest version present anywhere in the graph. In a workspace whose root pins `postcss: 8.5.10`, an importer that depends on `webpack` and declares no `postcss` of its own got `postcss@8.5.22` hoisted for `terser-webpack-plugin`'s optional `postcss` peer, leaving two `postcss@8.5.x` instances in the graph [#13320](https://github.com/pnpm/pnpm/issues/13320).

- A missing optional peer dependency is no longer satisfied by a prerelease version that its declared range doesn't accept. `ts-jest`, which declares `@jest/transform` and `jest-util` as optional peers with `^29.0.0 || ^30.0.0`, was bound to `30.0.0-alpha.6` when a `jest` 30 prerelease was elsewhere in the graph, while `jest` itself stayed on 29.

- With `autoInstallPeers: false`, a package's own optional peer dependencies are no longer added to its importer entry in `pnpm-lock.yaml` (and no longer linked into its `node_modules`) when another workspace project happens to resolve a matching version [#13325](https://github.com/pnpm/pnpm/issues/13325).

- `overrides` now also govern peers that pnpm auto-installs. Previously an override only rewrote dependencies declared in a manifest, so a peer nobody declares — installed because `autoInstallPeers` is on — resolved against its declared peer range and could bring in a second copy of the very package the override pinned. For example, with `overrides: { react: npm:react@19.2.0 }` and a lone `lucide-react` dependency, pnpm installed `react@18.3.1`; it now installs the pinned `react@19.2.0` [#13320](https://github.com/pnpm/pnpm/issues/13320).

- `pnpm approve-builds -g` is accepted again, reporting that the command is not supported with global packages rather than failing with `unexpected argument '-g' found`. `approve-builds` was the only command that declared `--global` without its `-g` short form [#13310](https://github.com/pnpm/pnpm/issues/13310).

- The Rust implementation of pnpm has moved from alpha to beta releases.

- A catalog name containing a control character no longer corrupts `pnpm-workspace.yaml`. `pnpm add --save-catalog-name "$(printf 'a\nb')"` (or the same value in `saveCatalogName`) now fails with `ERR_PNPM_WORKSPACE_MANIFEST_WRITER_INVALID_CONTROL_CHARACTER` and leaves the file untouched, matching how the writer already treats `allowBuilds` and `overrides` entries.

- Preserve each direct dependency's locked optional peer context during `pnpm dedupe`.

- Preserve optional peer providers recorded in peer suffixes when `pnpm dedupe` rebuilds a workspace lockfile.

- With `nodeLinker: hoisted`, a workspace project no longer gets its own copy of a dependency whose version already won the workspace-root slot. Only the versions that lost the root slot are nested, matching the pnpm CLI. Previously every project's direct dependency was materialized under that project as well, which gave lifecycle scripts a second copy to run in.

- The Rust engine now warns when `package.json` still declares install settings under the `pnpm` field, which pnpm 10 moved to `pnpm-workspace.yaml`. A project that hasn't migrated its `pnpm.overrides` / `pnpm.packageExtensions` / `pnpm.patchedDependencies` previously saw the settings silently ignored, and only met the downstream symptom. Keys the `pnpm` field never owned are left alone.

- Fixed `pnpm licenses list` and `pnpm licenses ls` parsing and license metadata discovery when using the global virtual store [pnpm/pnpm#13332](https://github.com/pnpm/pnpm/issues/13332) and [pnpm/pnpm#13333](https://github.com/pnpm/pnpm/issues/13333).

- A `pnpm-workspace.yaml` that declares a package pattern whose directory does not exist yet — `packages/*` before the first package is created, say — no longer fails every command with `ERR_PNPM_WORKSPACE_WALK_ERROR`. The pattern now matches no projects, as it does in the JavaScript implementation [#13296](https://github.com/pnpm/pnpm/issues/13296).

- Fixed `catalog:` references failing to resolve when installing through a pnpr server, which errored with "No catalog entry '<name>' was found for catalog 'default'." even though the catalog entry existed. The workspace the server reconstructs from the request has no catalog sections, so the client now sends its catalogs along with the request [#13232](https://github.com/pnpm/pnpm/issues/13232).

- Validate the project's pinned package manager and runtimes before running a command, matching the pnpm CLI:

  - A `packageManager` / `devEngines.packageManager` pin that the running pnpm does not satisfy now fails with `ERR_PNPM_BAD_PM_VERSION` (or `ERR_PNPM_OTHER_PM_EXPECTED` when the project is pinned to another package manager), instead of being silently ignored. The check also runs under corepack, where pnpm cannot switch versions itself, and says so.
  - `devEngines.runtime` / `engines.runtime` entries with `onFail: "error"` or `onFail: "warn"` are validated against the Node.js, Deno, or Bun installed on the system, failing with `ERR_PNPM_BAD_RUNTIME_VERSION`.
  - `pmOnFail` and `runtimeOnFail` are honored as bypasses and can now be passed as `--pm-on-fail=<value>` / `--runtime-on-fail=<value>`, the form the error hints suggest.

  Global commands (`--global`) and commands that do not belong to the project (`store`, `dlx`, `self-update`, …) skip these checks, as does a project pin that only asked pnpm to switch versions when `manage-package-manager-versions` is turned off.

- Arguments after the script or command name now reach the script untouched for `pnpm run`, `pnpm exec`, `pnpm dlx`, and `pnpm with`, matching the JavaScript implementation. Previously `pnpm run build --config.foo=bar` consumed the argument as a pnpm setting instead of forwarding it, and `pnpm run build --silent` handed the script `--reporter=silent` — a token the user never typed [#13302](https://github.com/pnpm/pnpm/issues/13302). Put such flags before the script name (`pnpm run --silent build`) to apply them to pnpm.

- Fixed generated lockfiles to preserve packages' scalar `libc` constraints.

- Preserve a user-provided `TMPDIR` when scripts run with `unsafePerm` enabled; otherwise, continue using the package-local temporary directory.

- Added support for `publishConfig.name`, which publishes a package under a different name than the one its manifest carries in the workspace. Only the published artifact is renamed — dependents, `pnpm-lock.yaml`, and release tooling keep addressing the project by its manifest name — and the new name reaches the packed manifest, the tarball filename, and everything that addresses the package at the registry: the already-published check of `pnpm publish -r`, its registry selection, and the release-planning probes of `pnpm change status` and `pnpm version -r`. This also fixes the changelog of the Rust CLI itself, which is published as `pnpm` from a workspace project named `pacquet`: its release notes were composed under the workspace name and so never made it into the published package [#13345](https://github.com/pnpm/pnpm/issues/13345).

- `pnpm run <script> <args>` now forwards every argument after the script name to the script verbatim, matching the behavior of the JavaScript implementation. Previously the `--` separator was dropped, so `pnpm run test -- --watch` reached the underlying program as `--watch` and failed whenever that program claimed the option itself; arguments spelled like `pnpm run`'s own flags (`-s`, `--if-present`) were also consumed by pnpm instead of reaching the script [#13295](https://github.com/pnpm/pnpm/issues/13295). Pass those flags before the script name (`pnpm run -s test`) to apply them to pnpm.

- `pnpm test`, `pnpm start`, and `pnpm stop` now forward their arguments to the script, matching the pnpm CLI. `pnpm test --watch` and `pnpm start --port 3000` previously failed with a usage error, and `pnpm stop` claimed `--if-present` and `-s` for itself instead of passing them on. As with `pnpm run`, every token after the command name reaches the script verbatim, a `--` separator included.

- Added the `--workspace-root` (`-w`) flag, which runs the command on the root workspace project. `pnpm add -D typescript prettier -w` from a workspace subdirectory now saves to the root `package.json` instead of failing with "unexpected argument '-w' found" [#13031](https://github.com/pnpm/pnpm/issues/13031). Combined with `--recursive`, the flag narrows the run to the root project alone. `-w` may not be used together with `--global`, and may only be used inside a workspace.

- `patchedDependencies` patch files that pnpm applies no longer fail with `ERR_PNPM_PATCH_FAILED`: a hunk whose last line is context in a file with no final newline, and an LF patch against a CRLF file, both apply again [#13322](https://github.com/pnpm/pnpm/issues/13322). A hunk that has drifted from its recorded line numbers is also retried nearby, matching pnpm.

- A peer dependency is now recorded in the lockfile at the version and peer suffix the peer provider actually resolved to. Peers whose provider carried peer suffixes of its own could be recorded against a package instance that no importer installs, leaving an unreachable entry in `snapshots:` and a peer bound to the wrong instance [#13320](https://github.com/pnpm/pnpm/issues/13320).

- A peer dependency that the workspace root already provides is no longer installed a second time. With `resolvePeersFromWorkspaceRoot` enabled (the default), a missing peer is matched against the **workspace root** project's dependencies; it was matched against the dependencies of whichever project was being resolved, so a project that didn't declare the peer itself resolved its own copy from the registry. In [vercel/next.js](https://github.com/vercel/next.js), whose `overrides` pin `react` to a single canary build, this pulled in a second `react` and paired it with `react-dom` from the canary — a combination the pin exists to prevent.

- Under `resolvePeersFromWorkspaceRoot`, a workspace root dependency declared with `link:` or `file:` (or the path form of `workspace:`, such as `workspace:../pkg`) now satisfies another project's missing peer dependency at the linked package's own version, instead of being hoisted as a path. Those specifiers are relative to the project that declares them, so the same specifier reached a different directory — or none — from the project the peer was hoisted into, leaving a broken link. The root now has the same authority over the peer as it has when it declares the package with a version range [#13373](https://github.com/pnpm/pnpm/issues/13373).

- Two `pnpm install` peer-resolution fixes that made large workspaces such as [Astro](https://github.com/withastro/astro) produce a different `pnpm-lock.yaml` than pnpm 11 [#13334](https://github.com/pnpm/pnpm/issues/13334):

  - A package that declares the same name in both `dependencies` and `peerDependencies` no longer gets a nested copy of it when the parent already supplies that name, which is what pnpm does with `autoInstallPeers` disabled. The nested copy hid the peer, so the package was recorded without the peer context it resolves in.
  - A duplicate peer-suffixed variant that collapses into a larger, compatible one now collapses everywhere it is referenced. A variant kept alive by a single consumer's edge no longer lingers in the lockfile.

- Closed the remaining gaps in how unscoped per-registry `.npmrc` settings are pinned to the registry their own source file declared:

  - An inline `cert=` / `key=` written with `\n` escapes now expands to a real multi-line PEM, matching the URL-scoped `//host/:cert=` spelling.
  - `pnpm config get` / `pnpm config list` now report a rescoped credential under the URL-scoped key it was pinned to, instead of the unscoped key it was written as.
  - The deprecation warning names the file it read and lists every setting it pinned, including `tokenHelper`.
  - A credential with no registry of its own is no longer attached to the resolved default registry, which repository config can move. The same rule now covers the `@pnpm/napi` bindings: the `authHeaderByUri` entry written with an empty (`""`) key is pinned to the `registry` / `registries.default` the host passed alongside it, never to a registry the project's `.npmrc` names.

- The projects that run their own lifecycle scripts (`preinstall`, `install`, `postinstall`, `prepare`, …) now match pnpm in every install-family command. A project runs them when the command installs it in full, and — in a workspace the command only partly covers — whenever the command mutates it at all; the workspace root runs them even when the command was pointed at another project, because it is installed in full alongside it. As a result, `pnpm update <pkg>` and `pnpm add <pkg>` in a workspace no longer skip the workspace root's scripts, `pnpm update` at a workspace root no longer runs the other members' scripts, and `pnpm update --latest` no longer runs the project's own scripts (it rewrites named dependency specs, so it is a partial install like `pnpm update <pkg>`) [#13358](https://github.com/pnpm/pnpm/issues/13358).

- Print the script command by default when running a filtered lifecycle script. The command remains hidden with `--silent`.

- Run dependency verification consistently after regenerating a lockfile with `dedupePeers` enabled.

- Aligned the `hoistedDependencies` contents and ordering in `node_modules/.modules.yaml` with pnpm.

- Preserve whether package `libc` metadata uses a string or an array when writing the lockfile.

- A `package.json` that starts with a UTF-8 byte order mark is read again instead of failing with `expected value at line 1 column 1`. Workspace discovery, dependency manifests (including bin linking), tarball extraction, and `pnpm publish` of a pre-built tarball all accept one, matching pnpm [#13311](https://github.com/pnpm/pnpm/issues/13311). A manifest that really is malformed now reports its path in the error.

- Fixed `ERR_PNPM_BROKEN_LOCKFILE` when installing with a pnpm 10 lockfile that has a `patchedDependencies` section. See pnpm/pnpm#13307.

- `pnpm -r run "/pattern/" --no-bail` no longer exits zero when one of a project's matched scripts fails and a later one passes. The run summary carries a single status per project, and the passing script overwrote the recorded failure.

- Resolution failures now report the error pnpm defines for them. A well-formed range that the registry publishes nothing for fails with `ERR_PNPM_NO_MATCHING_VERSION` — naming the latest release, the other dist-tags, and the `pnpm view <pkg> versions` command that lists the rest — instead of `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`. A package the registry doesn't have fails with `ERR_PNPM_FETCH_404` and the "not in the npm registry, or you have no permission to fetch it" hint (plus which authorization header was sent, since a private registry often answers a permission failure with a 404) instead of a bare HTTP-client message. A wrapper that quotes its cause verbatim no longer prints the same sentence twice in the error report.

- Fixed `pnpm add <workspace-package>` to resolve the local package when `linkWorkspacePackages` is enabled.

- `$dep-name` self-references in `overrides` are now resolved against the root manifest's direct dependencies, so an override such as `rolldown: $rolldown` records the concrete specifier in `pnpm-lock.yaml` and no longer fails a frozen install with `ERR_PNPM_OUTDATED_LOCKFILE` [#13314](https://github.com/pnpm/pnpm/issues/13314). A reference to a package that is not a direct dependency fails with `ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION`, and the deprecated syntax now warns, pointing at catalogs.

- The root project's `pnpm:devPreinstall` script now runs before resolution and linking, as it does in pnpm 11. It is skipped under `--ignore-scripts`, `--lockfile-only` and `--dry-run`, by `pnpm fetch` and `pnpm rebuild`, and by a repeat install that is already up to date. Workspaces that use the hook to prepare state the install depends on — such as [next.js](https://github.com/vercel/next.js), which generates a placeholder `next` bin with it — were left with dependents linked against files that were never created [#13313](https://github.com/pnpm/pnpm/issues/13313).

- The lockfile-verification line now dates a cached verdict — `✓ Lockfile passes supply-chain policies (verified 253ms ago)` — instead of the timeless `(previously verified)` [#13315](https://github.com/pnpm/pnpm/issues/13315).

- `pnpm install`, `run`, `test`, `update`, `remove`, `link`, `unlink`, `prune`, and `rebuild` now print the workspace scope they resolved — `Scope: all 41 workspace projects`, or `Scope: 5 of 41 workspace projects` under a `--filter`. This is the confirmation that a filter selected what was intended [#13315](https://github.com/pnpm/pnpm/issues/13315).

- An install that blocks a dependency's build scripts now appends a placeholder for it to `pnpm-workspace.yaml`, so approving or denying the build is an edit rather than writing the block by hand:

  ```yaml
  allowBuilds:
    es5-ext: set this to true or false
  ```

  A placeholder is not a decision — the build stays blocked until it is replaced with `true` or `false` — and an existing entry is never overwritten [#13315](https://github.com/pnpm/pnpm/issues/13315).

- Fixed `--parallel` being treated as the script name when placed before `run` in a recursive command.

- `scriptShell` now selects the shell for lifecycle scripts too — dependency build scripts and a project's own `preinstall`/`install`/`postinstall`/`prepare` and `pnpm:devPreinstall` — not only for `pnpm run` and `pnpm exec`. A workspace that configures a shell was still getting the platform default (`sh` / `cmd`) for everything the install itself spawns.

- Fixed `shamefullyHoist: true` to create public root dependency links.

- Prevented optional peers from being selected from an unrelated workspace package's shared dependency context.

- The store index now keys URL, git-host, and `type: git` dependencies by their bare resolution id, matching the key pnpm 11 writes [#13365](https://github.com/pnpm/pnpm/issues/13365). Previously these rows carried a `<name>@` prefix, so a store warmed by one pnpm major was cold for the other and every non-registry dependency was re-downloaded, re-extracted, and re-imported on a switch. A remote tarball also occupied two index rows instead of one, doubling its extraction work.

- A project that pins pnpm through `devEngines.packageManager` (or a v12+ `packageManager` field) now gets its `packageManagerDependencies` recorded in `pnpm-lock.yaml` by every command, not just by the install-family ones [#13348](https://github.com/pnpm/pnpm/issues/13348). Running `pnpm list` (or any other command) in a freshly cloned project no longer leaves the lockfile without the pinned version. The `pmOnFail` setting now also decides whether the pin is recorded: `--pm-on-fail=ignore` keeps it out of the lockfile even when the manifest asks for a stricter policy, and vice versa.

- Fixed `pnpm licenses list` to detect licenses from license files and preserve the latest package version's development classification.

- `pnpm update --latest` now rewrites `jsr:` dependencies. The manifest keeps the protocol and the range operator it declared, so `jsr:1.0.0` becomes `jsr:2.0.0` and `jsr:@scope/name@^1.0.0` becomes `jsr:@scope/name@^2.0.0`, instead of being left at the old version [#13363](https://github.com/pnpm/pnpm/issues/13363).

- `pnpm update --latest` now rewrites dependencies using a named registry alias. The manifest keeps the alias prefix and the range operator it declared, so `gh:1.0.0` becomes `gh:2.0.0` and `gh:@acme/foo@^1.0.0` becomes `gh:@acme/foo@^2.0.0`, instead of being left at the old version [pnpm/pnpm#13393](https://github.com/pnpm/pnpm/issues/13393).

- **Breaking change from pnpm v11.** Under `engineStrict`, an install fails when an incompatible package is reached through a regular `dependencies` edge of an installable package, even when that whole subtree hangs off an `optionalDependencies` entry. pnpm v11 installs the package and emits an install-check warning instead. Packages reachable only through optional edges, or through a package that was itself skipped, are still skipped in both versions [#13286](https://github.com/pnpm/pnpm/issues/13286).

- A lockfile entry whose tarball resolution records no `integrity` is now reported by the lockfile-verification gate, before anything is downloaded: every offending entry is listed in one `ERR_PNPM_MISSING_TARBALL_INTEGRITY` error instead of failing the install one fetch at a time after the gate had already passed the lockfile [#13364](https://github.com/pnpm/pnpm/issues/13364). An `integrity: ''` that pins nothing is treated the same as a missing one, and the exemption for git-host archive URLs is now read from the URL rather than the lockfile's own `gitHosted` marker.

- Fixed `pnpm licenses list` to read licenses from legacy package manifest fields.

- Fixed `--workspace-root` (`-w`) selecting the current workspace when `--dir` pointed at a nonexistent directory outside it (for example `pnpm --dir ../../elsewhere add -w foo`). The command now fails with `ERR_PNPM_NOT_IN_WORKSPACE`, matching pnpm. A nonexistent `--dir` inside the workspace still resolves to the workspace root as before.
