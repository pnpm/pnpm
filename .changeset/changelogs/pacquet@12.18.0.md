## 12.18.0

pnpm 12.18.0 packs, publishes, and installs packages with platform variants on pnpr registries. It also adds per-skill approval of agent skills and `pnpm store add` for offline installs from downloaded tarballs. Repeat installs and `verifyDepsBeforeRun` no longer read the `package.json` of unchanged workspace projects. The release fixes several peer dependency resolution bugs too.

### Minor Changes

#### Agent skills

- pnpm now names the link to a dependency's agent skill after the skill alone. A skill that the `@acme/kit` package ships in `skills/auth/` is linked as `.claude/skills/auth`, not `.claude/skills/pnpm-@acme+kit-auth`. The next install replaces links with the old names. pnpm lists the linked skills in a `# pnpm` block of each agent skill directory's `.gitignore` [#17455](https://github.com/pnpm/pnpm/discussions/17455).

  When two approved packages ship a skill of the same name, or the name is already taken in an agent skill directory, pnpm links the skill as `pnpm-<package>-<skill>`. Before, the install failed.

- `pnpm approve` now asks which agent skills to link when a package ships more than one. pnpm records the chosen skills as a list, such as `permissions.<pkg>.skills: [guide, migrate]`, and does not offer the other skills for approval again. The first time a project approves a skill, `pnpm approve` also asks which agent directories to link skills into, and records the answer under `skills.dirs`.

- pnpm links agent skills nested in category directories, such as `skills/engineering/implement/SKILL.md`, up to three levels below `skills/`. Before, it linked only the skills directly under `skills/`.

#### Platform variants

These features need a registry declared with `serverType: pnpr`.

- `pnpm pack` packs one archive per entry of `publishConfig.variants`, each platform variant as `<name>-<version>~<tag>.tgz`. `pnpm pack --variant <target>` packs only the archive whose entry lists that target. `pnpm pack --json` prints the `integrity` and `size` of every archive it writes.

- `pnpm publish` publishes a package's `publishConfig.variants`. pnpm uploads every archive before the publish request that references them. Publishing variants to any other registry is refused unless `--no-variants` is given, which publishes only the archive that targets `any`.

- `pnpm publish --upload-variant <target>` uploads the archive of one `publishConfig.variants` entry to the registry's staging area and prints `<target>=<integrity>`. `pnpm publish --variant-integrity <target>=<integrity>` then publishes the version from archives that other jobs uploaded. With `--recursive`, each value names its package as `<name>~<target>=<integrity>`.

- `pnpm publish -r --batch` now uploads each package archive on its own request and skips archives the registry already holds, so the batch request no longer grows with the archives it publishes. A batch of more than 256 packages still carries every archive inline. Packages with `publishConfig.variants` can be published in a batch. A batch with variants that would reference more than 256 archives on one registry fails with `ERR_PNPM_BATCH_PUBLISH_TOO_MANY_ARCHIVES` [pnpm/tasks#244](https://github.com/pnpm/tasks/issues/244).

- `pnpm publish --provenance` signs one statement that names every archive of a version with `publishConfig.variants`. A platform variant's archive is named by the version's package URL with a `variant` qualifier.

- Rerunning `pnpm publish` after the version was already published now succeeds when the published version is the same release. pnpm compares the archives, the provenance statement when the rerun signs one, and the dist-tag. When the published version differs, the publish fails with `ERR_PNPM_PUBLISHED_RELEASE_DIFFERS`, which names what differs.

- pnpm installs the platform variant of a package that fits the host, or the package's own archive when none does. The lockfile records every variant. A version published with variants only, and no archive for other hosts, installs on a host one of the variants fits. On any other host, an optional dependency is skipped, and any other dependency fails with an error that lists the platforms the version is published for.

- `supportedArchitectures` platform lists accept platform tags, the same tags `publishConfig.variants` entries name. An entry can state floors and an ABI, such as `linux-x64-manylinux_2_28`, `darwin-arm64-macos_14_0`, `linux-x64-napi8` or `linux-x64-node127`, and any Node.js OS and CPU, such as `freebsd-x64`. The spellings accepted before still work.

- With `supportedArchitectures` set, pnpm also fetches into the store the variants that the other listed platforms need, when it downloads a package with platform variants.

#### pnpr registries

- pnpm downloads less metadata from a registry declared with `serverType: pnpr`. A large package document leaves out the manifests of rarely picked versions, and pnpm fetches one only when it picks that version. In a benchmark that resolves 787 packages, the package documents shrank from 30.3 MB to 2.5 MB gzipped. pnpm caches these documents in `v12/metadata-pnpr`, and the `pnpm cache` commands cover them.

  pnpm warns when a registry that is not declared `serverType: pnpr` turns out to be pnpr. For a registry declared `serverType: pnpr`, a `minimumReleaseAge` longer than 30 days fails with `ERR_PNPM_UNSUPPORTED_MINIMUM_RELEASE_AGE`.

- `pnpm publish` sends release notes to registries declared with `serverType: pnpr`. A package's notes are the section for its version in `CHANGELOG.md`, or the notes that `pnpm version -r` composed from change intents. `--release-notes <file>` gives the notes from a file, and `--no-release-notes` sends none. `--publication-name` names a release published with `--batch`.

  In a workspace that uses pnpm's versioning, `pnpm version -r --release-notes <file>` puts a summary above the composed notes of the release.

  `pnpm view <pkg>@<version> releaseNotes`, `pnpm outdated --notes`, and `pnpm update --interactive` show the release notes from these registries.

#### Store and hooks

- `pnpm store add` now prepares the store for offline installs from package tarballs downloaded some other way. After `pnpm store add ./tarballs/*.tgz`, `pnpm install --offline --frozen-lockfile` installs each registry package whose lockfile entry pins one of those tarballs [#2978](https://github.com/pnpm/pnpm/issues/2978).

- Custom pnpmfile fetchers can use `fetchers.gitHostedTarball`, `fetchers.git`, and `fetchers.binary` [pnpm/tasks#191](https://github.com/pnpm/tasks/issues/191).

### Patch Changes

#### Installing packages

- Running `node` or `pnpm install` in a project that pins a Node.js version pnpm has not downloaded yet no longer hangs when a `globalPnpmfile` is configured [#17530](https://github.com/pnpm/pnpm/issues/17530).

- `pnpm install` with `enableGlobalVirtualStore` no longer fails on Windows for a dependency from a named registry, such as `gh:` or `npmjs:` [#17665](https://github.com/pnpm/pnpm/issues/17665).

- Concurrent installs that share a global virtual store no longer fail with `ERR_PNPM_CMD_SHIM_SYMLINK_BIN` when they link the same executable at the same time [#17587](https://github.com/pnpm/pnpm/issues/17587).

- `pnpm install` and `pnpm ci` now succeed when dependency skills are approved but no agent skill directory exists. The skills stay unlinked until a directory exists [#17574](https://github.com/pnpm/pnpm/issues/17574).

- Commands run from a workspace package now use the same default store as the workspace root. Before, `pnpm add` in a package could fail with `ERR_PNPM_UNEXPECTED_STORE` when the workspace root was its own mount point and the home store was on another volume, as in a sandbox or a container.

  A workspace installed with the old behavior may need one `pnpm install --force` from the workspace root.

- `pnpm deploy` now keeps a symlink in the project whose target is missing from the source, such as a link into a `node_modules` directory that was removed after the build. Deploy used to drop such a link silently [#17571](https://github.com/pnpm/pnpm/issues/17571).

- `pnpm install` now links the runtime that an earlier `pnpm install --no-runtime` skipped, and `pnpm install --no-runtime` removes a runtime that an earlier install linked [#17542](https://github.com/pnpm/pnpm/issues/17542).

- With `enableGlobalVirtualStore`, `pnpm install` again runs a dependency's build scripts in a project whose global virtual store slot another project already built, unless the side-effects cache has build output to restore for it. A build that writes only outside its own package, such as a git hook installer, now takes effect in every project [#17526](https://github.com/pnpm/pnpm/issues/17526).

- Commands of a package whose `bin` paths contain excess parent directory segments, such as `../dist/cli.js`, are no longer missing. Such paths now resolve inside the package [#17586](https://github.com/pnpm/pnpm/issues/17586).

- Generated POSIX shell shims now quote their paths, so shell special characters in a path stay literal.

#### Lockfile and local paths

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- `pnpm install --frozen-lockfile`, `pnpm approve-builds`, and `pnpm rebuild` now accept a lockfile that resolves a `*` dependency to a prerelease `latest` version. pnpm wrote such a lockfile and then reported it as outdated [#17594](https://github.com/pnpm/pnpm/issues/17594).

- `pnpm install --frozen-lockfile` now accepts a lockfile written with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a `catalog:` entry. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE`.

- `pnpm install --frozen-lockfile` no longer rejects a freshly generated lockfile when `addMissingPeerTypes` is enabled and an injected workspace package has peer dependencies [#17668](https://github.com/pnpm/pnpm/issues/17668).

- Relative local paths in root `overrides` now resolve from the workspace root when each project has its own lockfile [#17691](https://github.com/pnpm/pnpm/issues/17691).

  Installing a local directory dependency no longer fails when another project's install is writing a lockfile in that directory.

- Relative local paths in root catalogs now resolve for dependencies and peers [#17690](https://github.com/pnpm/pnpm/issues/17690).

- `pnpm install` now resolves auto-installed relative `workspace:` and `file:` peers from the directory of the package that declares them [#17676](https://github.com/pnpm/pnpm/issues/17676), [#17689](https://github.com/pnpm/pnpm/issues/17689).

- `pnpm dedupe --check` leaves `pnpm-lock.yaml` untouched while it looks for deduplication changes. If `configDependencies` changed since the last install, the check keeps the lockfile they sync, as `pnpm install` does [#17628](https://github.com/pnpm/pnpm/issues/17628).

- `pnpm dedupe --check` no longer fails when the only difference is the integrity of a locked version that the registry now reports with a stronger algorithm, such as a `sha512` integrity replacing the `sha1` shasum it reported when the lockfile was written [#17692](https://github.com/pnpm/pnpm/issues/17692).

#### Peer dependencies

- Peer deduplication no longer gives a project different copies of a dependency across direct and transitive paths when an optional peer has no provider [#17666](https://github.com/pnpm/pnpm/issues/17666).

- Dependencies no longer bind to another project's peer variant when the provider has nested peers, including providers that finish resolving later [#17593](https://github.com/pnpm/pnpm/issues/17593).

- Re-resolving a lockfile no longer preserves optional peer bindings from other projects when no provider is in scope [#17592](https://github.com/pnpm/pnpm/issues/17592).

- `pnpm peers check` warns when a required peer is resolved without a provider declared by the project or its dependency ancestors, with `autoInstallPeers: false` and `dedupePeerDependents: true`. These warnings do not change the command's exit code or cause `strictPeerDependencies` failures.

#### Speed and memory

- `pnpm install`, and `pnpm run` and `pnpm exec` with `verifyDepsBeforeRun`, no longer read the `package.json` of workspace projects whose files did not change since the last install. This also holds with a pnpmfile, `configDependencies`, or `sharedWorkspaceLockfile: false`. In an up-to-date workspace with `sharedWorkspaceLockfile: false`, `pnpm install` no longer runs the install of each project [pnpm/tasks#256](https://github.com/pnpm/tasks/issues/256).

- A recursive `pnpm run` or `pnpm exec` with `verifyDepsBeforeRun` set no longer reads every `package.json` in the workspace once per project when `sharedWorkspaceLockfile` is `false`. In a 300-project workspace, `pnpm -r exec true` went from 490 ms to 42 ms [pnpm/tasks#256](https://github.com/pnpm/tasks/issues/256).

- `pnpm install` makes fewer `stat` calls when it hardlinks files from the store, as few as 12.7.0 made [#17543](https://github.com/pnpm/pnpm/issues/17543).

- Picking versions from registry metadata uses less temporary memory.

#### verifyDepsBeforeRun and repeat installs

- With `verifyDepsBeforeRun`, `pnpm run` and `pnpm exec` now report dependencies as out of date after a dependency is added to the `package.json` of a local directory dependency, such as `"lib": "file:../lib"`. Before, the command ran without the new dependency installed.

- `verifyDepsBeforeRun` no longer reports a `file:` directory dependency as out of date after an edit to `package.json` that leaves its dependencies unchanged [#17697](https://github.com/pnpm/pnpm/issues/17697).

- `verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a single-project repository changed [#17560](https://github.com/pnpm/pnpm/issues/17560). The same holds for a workspace project that no dependency can resolve to. A script run after such a change no longer starts an install first.

- A repeat `pnpm install` now rewrites the bin shims after `extendNodePath` changes. Before, it reported "Already up to date" and the shims kept the `NODE_PATH` of the previous setting.

#### Adding and updating dependencies

- `pnpm add <pkg>` without a version now saves the range that the other workspace projects declare for the package, or the one a matching override sets. A `catalog:` reference still comes first, and the `tag` setting is used only when none of these exists [pnpm/tasks#191](https://github.com/pnpm/tasks/issues/191).

- `pnpm update` with only negated selectors, such as `pnpm update '!foo'`, now leaves the dependencies listed in `update.ignoreDeps` unchanged. Negated `crate:` and `pypi:` selectors, such as `!crate:sha2`, now exclude that dependency and update the rest [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243).

- `pnpm update --latest` keeps three kinds of Cargo dependencies where they are, unless the crate is named on the command line. Before, it moved each one to the newest release and left a workspace that did not build.

  - A renamed dependency, such as `tower-0-4 = { package = "tower", version = "0.4" }`, stays within its requirement if the workspace also declares the crate under its own name.
  - A crate replaced through `[patch.crates-io]` keeps its version, with a warning.
  - A crate whose newest release is older than its current version, such as a prerelease newer than the latest stable release, keeps its version, with a warning. Only an explicit version, such as `pnpm update crate:foo@1`, moves it back.

  Other new major versions are still selected and may need code changes.

- `pnpm update` now keeps a Cargo dependency at its current version, with a warning, if the new version lacks a feature the workspace enables on it. If the crate is named on the command line, as in `pnpm update --latest crate:sha2`, the update fails with an error that names the missing features.

- `pnpm update` now keeps the operator of every existing Cargo requirement, so `~1.0.0` is updated to `~1.1.0`, and an exact `=` requirement stays exact unless the selector gives a range, such as `crate:foo@^2`. The `savePrefix` and `saveExact` settings apply only to new Cargo dependencies. Saved Cargo requirements omit build metadata.

- `pnpm update` now updates a Cargo dependency whose inline table spans several lines, such as one with a multi-line `features` array. It failed with "its Cargo.toml dependency declaration is not a string or single-line inline table".

- `pnpm update` no longer applies the `savePrefix` and `saveExact` settings to Python requirements. A `savePrefix` such as `^` made `pnpm update` fail in a project with Python dependencies.

  `pnpm update` now keeps the precision of a `~=` requirement, so `~=1.0` is updated to `~=1.5` and still allows any 1.x release. With `--latest`, a `==1.*` requirement is updated to `==2.*`.

#### Store

- `pnpm store prune` now keeps packages linked from a configured modules directory [pnpm/tasks#230](https://github.com/pnpm/tasks/issues/230).

- `pnpm store prune` now removes aliases of pruned packages.

- `pnpm store status` no longer reports untouched packages as modified when the global virtual store is enabled. If the installed global-store layout cannot be verified, the command reports `ERR_PNPM_STORE_STATUS_UNVERIFIABLE` and asks for a reinstall [#17631](https://github.com/pnpm/pnpm/issues/17631).

- `pnpm store add` now fails with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to write package files into the read-only store without recording them, so no install could use them.

- `pnpm cat-index <name>@<version>` no longer shows the index of a package whose name differs only in letter case.

#### Other commands and settings

- `pnpm link` now fails without changing `package.json` or `pnpm-workspace.yaml` when the linked package's name is not a valid npm package name. It used to write that name to both files, which made every later install fail [#17475](https://github.com/pnpm/pnpm/issues/17475).

- `pnpm link <dir>` now resolves a relative `<dir>` against the current directory. In a directory without a `package.json`, such as a workspace's `packages` folder, `pnpm link .` used to link the nearest project above it into itself [#17475](https://github.com/pnpm/pnpm/issues/17475).

- A Node.js version range such as `>=26.13`, for example in `devEngines.runtime`, now resolves to the newest stable release that satisfies it. pnpm picks a prerelease only when no stable release matches [#17516](https://github.com/pnpm/pnpm/issues/17516).

- Switching to the pnpm version a project pins no longer writes a temporary `_tmp_*` file into the project directory, which file watchers reported on every command [#17681](https://github.com/pnpm/pnpm/issues/17681).

- pnpm now reads the `linkWorkspacePackages`, `injectWorkspacePackages`, `extendNodePath`, `frozenStore`, and `optimisticRepeatInstall` settings from `PNPM_CONFIG_*` environment variables, such as `PNPM_CONFIG_LINK_WORKSPACE_PACKAGES=true`. `--config.extend-node-path` now applies too.

- Recursive scripts run with `--sequential` or `--workspace-concurrency=1` print GitHub Actions log groups again [#17629](https://github.com/pnpm/pnpm/issues/17629).

- When `pnpm docs` cannot open a browser, the URL it prints no longer includes the query string or the fragment.
