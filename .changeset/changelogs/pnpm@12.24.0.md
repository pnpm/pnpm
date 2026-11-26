## 12.24.0

This release adds `pnpm audit deprecations` and `pnpm audit engines`, moves install warnings into a checks footer above `Done in`, and lets global installs link approved agent skills. It carries a security fix: pnpm now reads every package tarball the way npm does, or refuses it. `pnpm approve <pkg>` also no longer grants every permission to a package that has no pending request.

### Minor Changes

- Added `pnpm audit deprecations`, which lists the installed packages that their registry marks as deprecated, with the projects or packages that bring each one in. It exits with code `1` if any package is deprecated. The deprecated line at the end of an install now points to this command.

- Added `pnpm audit engines`, which lists the installed packages that don't support the current Node.js version or platform, with what each one wants and the projects or packages that bring it in. It exits with code `1` if any package is incompatible. The lines about unsupported Node.js versions and platforms at the end of an install now point to this command.

- An install now ends with a checks footer above `Done in`. It lists deprecated packages, peer dependency issues, unsupported engines and platforms, the lifecycle scripts that ran, and the lockfile's supply-chain verdict. In a terminal, the footer is shown live below the progress while the install runs.

  Deprecated packages are listed on one line, such as `✗ 2 packages are deprecated: request@2.88.2 and uuid@3.4.0`. Peer dependencies with issues are named the same way, with a hint to run `pnpm peers check` for details.

  Dependencies that don't support the current Node.js version or platform are grouped by what they want, such as `✗ 15 packages want Node.js ^18.18.0 || ^20.9.0 || >=21.1.0, not 12.0.0: …`. When packages declare different Node.js ranges, the line shows the versions that satisfy all of them. pnpm no longer prints a warning for each of these packages.

  One line names the project scripts that ran, one the dependencies that were built, and one the dependency builds that came from the side-effects cache. A dependency that was skipped because its optional build failed is listed as a problem. While scripts run, their line shows a spinner and names the running scripts. A script that fails prints its output, as before. In CI and piped output, a script that succeeds without printing anything no longer prints any lines.

  The `pnpm:install-check` log event of an unsupported engine or platform now carries the error `code`, the `pkgId`, and what the package wants in `wanted`. An unsupported engine event also carries the `current` versions.

- The install summary prints an updated dependency on one line, such as `↑ lodash 4.17.20 → 4.18.1`. A downgrade is marked with `↓`. The changed part of the new version is colored the way `pnpm outdated` colors it.

  In a workspace, the summary names the projects behind each change only when the changes were made by different projects. A change made by one project shows its package name, such as `in @scope/app`. A change made by several projects shows how many, such as `in 5 projects`.

  Dependencies with newer versions are named on one line at the end, with a hint to run `pnpm outdated`. Summary lines no longer mark deprecated packages or end with "(x is available)". "Already up to date" is only printed when the install changed nothing.

- When an install finishes, its progress lines are replaced by the stats, such as `Packages: +58 -2 (90% downloaded)`, with the share of packages downloaded or reused from the store. In the bar below it, a downloaded package is drawn as `↓` and a reused one as `+`.

  The `Scope:` line is printed only when a filter selected some of the workspace projects, not when the command ran on all of them.

- Global installs can now link approved agent skills into skill directories under your home directory. Use `pnpm approve -g` to review skills and build scripts for globally installed packages [pnpm/tasks#117](https://github.com/pnpm/tasks/issues/117).

- `pnpm install` warns when a locally declared package permission matches no dependency during a full resolution. The permission stays saved for future installs. Set `permissions.unused` to `ignore` alongside `permissions.rules` to turn off the warning [#18429](https://github.com/pnpm/pnpm/issues/18429).

- `pnpm-workspace.yaml` accepts grouped forms of the override and catalog settings. `overrides.rules` holds override selectors alongside `overrides.unused` (`warn` or `ignore`). `catalog.entries` holds default catalog entries alongside `catalog.unused` (`ignore` or `prune`). `catalog.mode` and `catalog.defaultSaveTarget` configure how `pnpm add` uses catalogs and where it saves new entries. The existing map forms and flat settings remain supported [#18429](https://github.com/pnpm/pnpm/issues/18429).

- Package tarball downloads can use credentials over plain HTTP when the user's global settings explicitly declare the HTTP registry URL [#18427](https://github.com/pnpm/pnpm/issues/18427).

- A single-package version bump now allows unstaged edits and untracked files excluded from the package. Staged changes still block the bump [#18417](https://github.com/pnpm/pnpm/issues/18417).

### Patch Changes

#### Security and approvals

- pnpm now reads every package tarball the way npm does, or refuses it. A tarball whose headers npm could read differently, such as one with a malformed PAX header, could install different files with pnpm than with npm. A directory entry with a nonzero size and a PAX header that sets one key twice are now read as npm reads them. pnpm now extracts contiguous file entries in a tarball, as npm does.

- `pnpm approve <pkg>` no longer grants build scripts and agent skills to a package that has no pending request. It now asks which permissions to grant. In CI or without an interactive terminal, configure `permissions` or use `pnpm approve-builds` to grant build permission in advance [#18415](https://github.com/pnpm/pnpm/issues/18415).

- `pnpm approve` now lets you approve a Git dependency across updates. Press R on the package in the interactive prompt or pass its package name and repository URL without a commit [#18375](https://github.com/pnpm/pnpm/issues/18375). A bare package name or Git ref that cannot match a pending Git dependency now fails with the valid selectors, and no decisions are saved [#18388](https://github.com/pnpm/pnpm/issues/18388).

- Fixed exact versions with SemVer build metadata in `minimumReleaseAgeExclude`, `trustPolicyExclude`, and `allowBuilds`. Pruning now keeps exclusions for versions in the lockfile [#18444](https://github.com/pnpm/pnpm/issues/18444).

#### Installing packages

- Fixed `pnpm install --offline --frozen-lockfile` failing the release-age check when workspace manifests are copied after `pnpm fetch` [#18409](https://github.com/pnpm/pnpm/issues/18409).

- `pnpm install` now resolves a `catalog:` peer dependency declared by a local `file:` dependency. Installing such a dependency failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` [pnpm/tasks#310](https://github.com/pnpm/tasks/issues/310).

- Fixed `pnpm install` failing or writing an incomplete lockfile key when a `file:` dependency path contains parentheses [#18443](https://github.com/pnpm/pnpm/issues/18443).

- Fixed frozen installs rejecting a local dependency when the workspace override and lockfile use equivalent relative paths [#18381](https://github.com/pnpm/pnpm/issues/18381).

- Fixed concurrent installs sharing a store occasionally copying an empty or incomplete package file [#18433](https://github.com/pnpm/pnpm/issues/18433).

- Fixed repeated installs before `pnpm run` and `pnpm exec` after a scope registry changes when the command would run a frozen or filtered install [#18448](https://github.com/pnpm/pnpm/issues/18448).

- Fixed `pnpm deploy --prod --legacy` failing on patches for packages excluded from the deployment [#18394](https://github.com/pnpm/pnpm/issues/18394).

#### Resolving and linking dependencies

- Fixed missing commands in `node_modules/.bin` when a patched dependency moves to a new slot in the global virtual store [#18398](https://github.com/pnpm/pnpm/issues/18398).

- Fixed packages in the global virtual store resolving an injected workspace dependency from another checkout [#18447](https://github.com/pnpm/pnpm/issues/18447).

- Fixed `pnpm install` leaving a hidden hoist link behind when the workspace root adds the package as a direct dependency [#18362](https://github.com/pnpm/pnpm/issues/18362).

- Fixed `overrides` accepting selectors with multiple parents when a middle package had a version range [#18445](https://github.com/pnpm/pnpm/issues/18445).

- Fixed `pnpm update` overwriting Node.js version ranges such as `>=20 <23` in `engines.runtime` and `devEngines.runtime` with an exact version [#18360](https://github.com/pnpm/pnpm/issues/18360).

- Dependency changes now preserve the runtime policy declared in `package.json` when `runtimeOnFail` is set [#18376](https://github.com/pnpm/pnpm/issues/18376).

#### Platforms and the pnpm binary

- Fixed the FreeBSD x64 build of `pnpm`, which failed to compile.

- The pnpm binary is 10 to 13 MB smaller on Linux and macOS. It no longer includes the symbol names that only debuggers use.

- On Unix, scripts started through Corepack now receive signals sent to the pnpm PID. This requires Node.js 22.15 or later on the 22.x line, or Node.js 23.11 or later [#18408](https://github.com/pnpm/pnpm/issues/18408).

#### Commands and settings

- `pnpm info` and `pnpm view` now report a fully unpublished package with `ERR_PNPM_PACKAGE_NOT_FOUND`. They used to fail with a metadata decoding error, which stopped tools such as Changesets from checking the rest of a release. `pnpm add --save-types` now skips a `@types` package that was fully unpublished [#18428](https://github.com/pnpm/pnpm/issues/18428).

- Fixed `pnpm sbom --dev` omitting optional dependencies of development dependencies [#18403](https://github.com/pnpm/pnpm/issues/18403).

- Downloaded Node.js, Deno, and Bun runtimes now declare their MIT license in `package.json`. This lets `pnpm licenses list` report the runtime's declared license [#18431](https://github.com/pnpm/pnpm/issues/18431).

- pnpm now treats an empty `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, or `PNPM_HOME` as unset and uses the default directory. An empty value made pnpm read and write its global config, auth file, and cache in a `pnpm` directory under the current working directory.

- pnpm now expands a leading `~/` in `cacheDir` values loaded from the global config or `PNPM_CONFIG_CACHE_DIR` to the user's home directory, as it does for `storeDir`.

- `pnpm config set` now writes values containing Unicode line separators (U+2028 and U+2029) to `pnpm-workspace.yaml` unchanged, so reading the setting back returns the value that was set.

- When `pnpm-workspace.yaml` mostly uses double quotes, quoted values pnpm adds to it now use double quotes, such as the entries written by `pnpm config set`, `pnpm approve-builds`, and catalog updates. Previously only `minimumReleaseAgeExclude` and `trustPolicyExclude` entries followed the file's quote style [#18364](https://github.com/pnpm/pnpm/pull/18364).

- `pnpm install` no longer warns about a project's own `pnpm-workspace.yaml` when the workspace root already sets every setting in it. A project that is also installed on its own, such as a git submodule, can keep its `pnpm-workspace.yaml` without the warning [#18441](https://github.com/orgs/pnpm/discussions/18441).

#### Install output

- The install summary of a project with no previous install lists only the dependencies the command added or changed. A fresh `pnpm install` of a single project no longer lists every direct dependency, which matches how workspace projects were already reported. In a workspace, the summary again lists a dependency added to a project whose previous install had no dependencies.

- `pnpm install --lockfile-only` shows its progress again, counting the packages it resolves.

- An install from a project inside a workspace with a shared lockfile no longer prefixes its progress lines with the path to the workspace root. In CI and piped output, a large download now prints one line when it finishes, without the `0.00 B` line at its start.
