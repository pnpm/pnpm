## 12.0.0-rc.9

### Major Changes

- A project's `pnpm-workspace.yaml` may no longer carry a setting pnpm does not recognize. Such a setting used to be ignored in silence — a misspelled `minimumReleaseAge` dropped the policy it was meant to set, and nothing said so. Now it is reported, suggesting the closest real setting name when the key looks like a typo, and it fails the command with `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS` when the project pins a pnpm version the running pnpm satisfies: with the pin honored, the setting cannot be meant for a different pnpm version, so it is a mistake to fix rather than a key to ignore. Everywhere else it is a warning, so a project that has yet to be cleaned up keeps working.

  The `pnpm config` subcommands never fail on such a setting, so a broken file can still be inspected and repaired, and `pnpm config get <key>` prints the value with no warnings at all. Keys the global config file cannot set are likewise split between workspace-only settings (still directed to `pnpm-workspace.yaml`) and settings unknown to this version.

### Minor Changes

- Added global build approvals [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- Added recursive global outdated checks [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- `pnpm config get` and `pnpm config list` now show the settings pnpm acts on under their documented names:

  - `registries` shows the registries pnpm resolves from, merged across every source (`.npmrc`, `pnpm-workspace.yaml`, the global config, CLI flags), in the shape the setting is written in: keyed by registry URL, with the default registry declared as the bare `@` scope. Built-in routes are included — the `@jsr` scope and the `npmjs` and `gh` prefixes — unless pointed elsewhere. Previously `pnpm config get registries` printed `undefined`.
  - `update` and `audit` show the effective sections, whichever spelling set them. The deprecated internal spellings (`updateConfig`, `auditConfig`, `auditLevel`) are no longer listed.
  - `catalogs` shows the complete resolved catalog set — the singular `catalog` block is its `default` entry — whichever spelling declared it.
  - The `registry` and `@scope:registry` entries show the merged routes rather than raw `.npmrc` values, so they always agree with the `registries` view.

- Added support for configuring `stateDir` in the Rust pnpm CLI [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added bounded workspace concurrency for recursive run and exec commands [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- `@pnpm/napi` gained reporter output, reverse dependency queries, and lockfile access.

  `install` and `rebuild` accept `options.reporter` and render pnpm's terminal output — progress line, packages-diff summary, lifecycle output, and the `Done in …` footer. Rendered output goes to stdout, or to an `onOutput` callback for a host that writes its own output through JavaScript. New reporting options: `hideLifecycleOutput`, `ignoredBuildsInstructionText`, and `hideLinkedPkgsDiff`.

  `getDependents` returns the reverse dependency trees behind `pnpm why`, annotated with the `package.json` fields named in `manifestFields`. `renderDependents` returns those trees rendered as tree, parseable, or JSON output.

  `readLockfile` and `writeLockfile` read and write `pnpm-lock.yaml` (or the current lockfile under the virtual store). `filterLockfileByImporters` returns a lockfile narrowed to what the named importers reach. `readModulesManifest` returns the `.modules.yaml` state of an installed `node_modules`.

  Top-level lockfile keys pnpm does not define are no longer dropped when a lockfile is loaded and saved, so state a tool records beside pnpm's own keys survives a rewrite.

- `pnpm` now supports per-branch lockfiles in its Rust engine:

  - `gitBranchLockfile` gives each git branch its own `pnpm-lock.<branch>.yaml`, so two branches can hold different resolutions without conflicting on one file. A branch that has no lockfile yet installs against the shared `pnpm-lock.yaml`.
  - `mergeGitBranchLockfiles` (and the `--merge-git-branch-lockfiles` flag on `pnpm install`) folds every branch lockfile back into `pnpm-lock.yaml` and deletes them, which is what merging a branch into the mainline needs.
  - `mergeGitBranchLockfilesBranchPattern` (and `--merge-git-branch-lockfiles-branch-pattern`) names the branches that merge automatically, so a mainline branch does not have to pass the flag by hand [#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added `PNPM_CONFIG_VIRTUAL_STORE_ONLY` and `PNPM_CONFIG_ENABLE_MODULES_DIR` support to the Rust pnpm CLI.

- Added support for the `lockfileDir` setting and its `--lockfile-dir <dir>` flag on `pnpm install`, `add`, `update`, and `remove`. `pnpm-lock.yaml`, the root `node_modules` holding the virtual store, and the config dependencies now live in the given directory, each project is recorded under its path relative to it, and every project keeps its own `node_modules` of symlinks — so several projects can share one lockfile [#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added support for the `preferSymlinkedExecutables` setting. On POSIX systems, `node_modules/.bin` entries are created as symlinks to the executable files instead of shell shims, and `NODE_PATH` pointing at the virtual store of the workspace root is exported to spawned scripts so they can resolve dependencies from the hoisted store. Like the TypeScript CLI, the setting turns on automatically when `nodeLinker` is set to `hoisted`.

- Added the six CLI flags the TypeScript pnpm CLI accepts but the Rust CLI did not [#14101](https://github.com/pnpm/pnpm/issues/14101):

  - `--stream` prints a recursive command's script output as it arrives, one line at a time, prefixed with the project it came from, instead of letting the scripts write to the terminal directly. `--parallel` implies it, as in pnpm.
  - `--aggregate-output` holds each script's streamed output until the script exits and then prints it as one block, so concurrent projects can't interleave.
  - `--reporter-hide-prefix` drops that project prefix from the scripts' own output lines. On a recursive `pnpm exec`, the opposite spelling `--no-reporter-hide-prefix` turns the prefixing on.
  - `--use-stderr` sends the reporter's output to stderr, leaving stdout for the command's own result.
  - `--ignore-workspace` runs the command as if the project were standalone: no workspace root is discovered, so `pnpm-workspace.yaml` contributes neither settings nor sibling projects, and a blocked dependency build is not scaffolded into its `allowBuilds`.
  - `--workspace-packages` overrides the `packages` patterns of `pnpm-workspace.yaml` for the run.

  The `stream`, `aggregateOutput`, `reporterHidePrefix`, `useStderr`, and `ignoreWorkspace` settings are now read from `pnpm-workspace.yaml`, the global `config.yaml`, and their `PNPM_CONFIG_*` environment variables too.

- Added support for the `shellEmulator` setting. With it enabled, the scripts `pnpm run` executes, a project's own lifecycle scripts, and dependencies' build scripts run in a built-in POSIX shell instead of the platform's (`sh -c`, or `cmd /d /s /c` on Windows), so scripts written for `sh` behave the same on every OS. `scriptShell` is not used while the emulator is on.

- The Rust engine now checks that a package read back from the store is the package it was recorded as. When the tarball's `package.json` names a different name or version than the store entry was keyed for — a broken lockfile, or a registry serving content that doesn't match its metadata — the install fails with `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE`. Set the new `strictStorePkgContentCheck` setting to `false` to downgrade the failure to a warning and install from the entry anyway [#12042](https://github.com/pnpm/pnpm/issues/12042).

- `pnpm` now supports three workspace settings in its Rust engine:

  - `includeWorkspaceRoot` (and the universal `--include-workspace-root` / `--no-include-workspace-root` flags) keeps the workspace root project in a recursive `run`, `exec`, `add`, or `test`, which otherwise leave it out.
  - `ignoreWorkspaceCycles` and `disallowWorkspaceCycles` control the report an install makes when workspace projects depend on each other in a cycle: it is a warning by default, an `ERR_PNPM_DISALLOW_WORKSPACE_CYCLES` error under `disallowWorkspaceCycles`, and silent under `ignoreWorkspaceCycles` [#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added support for the remaining pnpm default settings, including recursive command controls, optional dependency selection, workspace-root checks, color modes, lockfile compatibility, and pack manifest options.

- Batch workspace publishing accepts a shared scope-specific credential, rejects mismatched credentials for a registry before publishing, and runs the `publish` and `postpublish` scripts after each completed registry group [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- Added the commands the Rust CLI was still missing:

  - `pnpm get <key>` and `pnpm set <key> <value>` — the top-level spellings of `pnpm config get` and `pnpm config set`.
  - `pnpm store status` — reports the packages whose files no longer match the store they were expanded from, failing with `ERR_PNPM_MODIFIED_DEPENDENCY`; and `pnpm store add <pkg>...` — fetches packages into the store without writing a manifest, a lockfile, or `node_modules`.
  - `pnpm env use --global <version>` and `pnpm env list [<selector>]`, the deprecated Node.js-only front end to `pnpm runtime`.
  - `pnpm edit`, `pnpm profile`, `pnpm token`, and `pnpm xmas` now fail with `ERR_PNPM_NOT_IMPLEMENTED` pointing at the npm CLI, instead of being taken for a package script.

- An install that resolves the dependency graph now reports the unmet peer dependencies it leaves behind, matching the TypeScript CLI. By default it warns once — `Issues with peer dependencies found. Run "pnpm peers check" to list them.` — and with `strictPeerDependencies` it fails with `ERR_PNPM_PEER_DEP_ISSUES` after the artifacts are written, listing every unmet peer. This covers `pnpm install`, `add`, `remove`, `update` and `--lockfile-only`; `pnpm dedupe` reported the same verdict already, and now shares the reporting with them. `peerDependencyRules` are applied before the verdict, so a rule that covers every issue leaves nothing to report, and a `--filter`ed install reports only on the projects it installed. An install that skips resolution — a frozen install, or one whose `pnpm-lock.yaml` is already up to date — reports nothing, as in the TypeScript CLI; `pnpm peers check` inspects such a tree [#14098](https://github.com/pnpm/pnpm/issues/14098).

- Added `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps` to the Rust pnpm CLI and its N-API bindings. Slow registry metadata requests and tarball downloads now emit pnpm-compatible warnings without exposing URL credentials, query parameters, fragments, or control characters [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added filtered and split SBOM generation with per-project lockfiles, including reachable workspace projects and incomplete-graph validation [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

### Patch Changes

- Kept pending build approvals available after removing an unrelated dependency.

- Fixed resolving the `chcp` command on Windows during `pnpm setup` by looking for `chcp.com` before `chcp` [pnpm/pnpm#13991](https://github.com/pnpm/pnpm/issues/13991).

- A custom fetcher can no longer replace the archive integrity that `pnpm-lock.yaml` pins: the locked value is restored after a `canFetch` or `fetch` hook rewrites the resolution, and delegating a locked archive to a directory or git source now fails instead of installing unverified content.

  The Rust CLI now also loads the pnpmfiles named by the `pnpmfile` setting (a single path or an ordered list), and hands custom fetchers native `localTarball` and `remoteTarball` callbacks — including on a fresh install that has to compute a missing tarball integrity, which is then reused by later offline installs. File maps a fetcher returns are accepted only when they match what those native callbacks extracted.

- `pnpm dedupe` accepts the `pnpm install` options that pnpm documents for it — `--lockfile-only`, `--ignore-scripts`, `--offline`, and `--prefer-offline` — instead of rejecting them with `unexpected argument`. Without `--lockfile-only`, `pnpm dedupe` now also updates `node_modules`, as an install does [#14107](https://github.com/pnpm/pnpm/issues/14107).

- `pnpm dedupe` in the Rust engine now fails with `ERR_PNPM_PEER_DEP_ISSUES` when `strictPeerDependencies` is set and unresolved peer dependency issues remain after deduplication, matching the TypeScript CLI [#14099](https://github.com/pnpm/pnpm/issues/14099). Previously it only ever printed a warning, regardless of the setting.

- `pnpm deploy --prod` and `pnpm deploy --no-optional` no longer list the excluded dependency groups in the deployed `package.json` and `pnpm-lock.yaml`. The deployed lockfile referenced packages that the deploy left out of its graph, so installing in the deploy directory afterwards created dangling symlinks [#13623](https://github.com/pnpm/pnpm/issues/13623).

- `pnpm install --dev` and `pnpm deploy --dev` no longer install optional dependencies, and `--prod` now takes precedence when combined with `--dev`, matching the TypeScript pnpm CLI.

- A dependency published with `"bin": ""`, such as `url-loader@1.1.2`, no longer fails the install with `ERR_PNPM_CMD_SHIM_PROBE_SHIM_SOURCE` [#13962](https://github.com/pnpm/pnpm/issues/13962). An empty `bin` declares no command, as it does in pnpm v11, so no shim is written for the package; a `directories.bin` entry on the same package is still linked.

- A dependency pinned to an exact version carrying semver build metadata (`"@parcel/codeframe": "2.0.0-canary.1718+d8408010f"`) installs again instead of failing with `ERR_PNPM_NO_MATCHING_VERSION` [#14096](https://github.com/pnpm/pnpm/issues/14096). npm strips build metadata when it publishes a version, so pnpm strips it from the version it looks up, matching npm and pnpm v11.

- A package's `files` entries now match only at the package root, the way npm reads them. A bare `src` used to also match nested directories such as `example/src`, so a dependency installed from git could ship the repository's own example app. The same filter decides what `pnpm pack` and `pnpm publish` put in a tarball and what `pnpm deploy` copies, so those stop carrying the extra files too. Exclusions such as `!**/__tests__` and `!*.map` still match at any depth. A package already in the store keeps its old file set until it is fetched again.

- A `pnpm install --filter <selector>` run that has nothing to do now reports "Already up to date" without entering the install pipeline, the same way an unfiltered `pnpm install` already did [#14033](https://github.com/pnpm/pnpm/issues/14033).

- On Windows, upgrading pnpm no longer leaves a stale `pnpm.ps1` behind. PowerShell resolves `pnpm.ps1` ahead of `pnpm.cmd`, so a shim written by an older installation kept running the previous version. Linking the pnpm CLI's bins now deletes it [#13919](https://github.com/pnpm/pnpm/issues/13919).

- Settings written to a `pnpm-workspace.yaml` block that uses inline (flow) YAML — `catalog: { foo: ^1.0.0 }`, `overrides: { foo: 1.0.0 }`, `minimumReleaseAgeExclude: [foo@1.0.0]` — are now edited in place instead of failing or corrupting the file. `pnpm audit`, `pnpm link`, `pnpm approve-builds`, `pnpm patch`, `pnpm add --config`, and catalog updates all keep the block's flow style, its other entries, and its comments [#14108](https://github.com/pnpm/pnpm/issues/14108).

- A frozen install no longer rewrites the `packageManagerDependencies` block of `pnpm-lock.yaml`. When the pnpm version pinned by `devEngines.packageManager` (or by `packageManager`) is missing from the lockfile or no longer matches it, `--frozen-lockfile` now fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` instead of resolving the version and saving it, so a manifest whose pin was bumped without regenerating the lockfile can no longer pass CI [#14009](https://github.com/pnpm/pnpm/issues/14009).

- When a git-hosted dependency is blocked from running build scripts, the error now suggests an `allowBuilds` entry that actually approves it. It quoted the bare package name, which never matches a git-hosted package, so following the suggestion left the install failing the same way [#14002](https://github.com/pnpm/pnpm/issues/14002).

- A git dependency installed over HTTPS from a hosted repository now keeps its branch, tag, or version range in the specifier recorded in `package.json`. It was written back without one, so the next `pnpm update` moved the dependency to the repository's default branch [#13999](https://github.com/pnpm/pnpm/issues/13999).

- Added support for the `globalPnpmfile` setting, which names a user-level pnpmfile that runs for every project ahead of the project's own. Like pnpm, it is left out of the lockfile's `pnpmfileChecksum`, so editing it does not decide whether a lockfile is still current. `pnpmfile` and `globalPnpmfile` are now also readable from `PNPM_CONFIG_PNPMFILE` and `PNPM_CONFIG_GLOBAL_PNPMFILE`.

- Fix recursive `pnpm update <name>@<version>` so an exact pinned update stays scoped to the requested version line: copies of the same package on another major line — or, for a `0.x` request, another minor line — keep their locked resolution instead of being re-resolved along with the target.

- Under `nodeLinker: hoisted`, a dependency declared against a peer-resolution variant of a package version is no longer dropped from the installed layout. All variants of a version share one hoisted copy, and edges pointing at any of them now resolve to it, so the depending project keeps the package in its `.package-map.json` and the depending package keeps it in its `node_modules/.bin`.

- A repeat `pnpm install` with `nodeLinker: hoisted` is a no-op again when a workspace package declares the dependencies [#14001](https://github.com/pnpm/pnpm/issues/14001). The hoisted linker installs them into the root `node_modules`, but the up-to-date check previously looked under each package's own `node_modules` and reinstalled the whole tree every time. A hoisted install also no longer reports the packages it just wrote as broken.

- `ignorePnpmfile` can now be set in `pnpm-workspace.yaml` and read from `PNPM_CONFIG_IGNORE_PNPMFILE`, not only passed as `--ignore-pnpmfile`, so a project or a machine can turn pnpmfile hooks off once instead of adding the flag to every command. The flag still applies on top. As in pnpm, the global `config.yaml` cannot set it: a pnpmfile belongs to the project that ships it.

- Fixed pnpm failing to read `.modules.yaml` files containing long dependency paths [#13875](https://github.com/pnpm/pnpm/issues/13875). The manifest is now parsed as JSON (the format pnpm writes it in), falling back to the YAML parser only for manifests written by old pnpm versions.

- `--config.minimum-release-age` is honored again, along with `--config.minimum-release-age-exclude`, `--config.minimum-release-age-ignore-missing-time` and `--config.minimum-release-age-strict`. Each overrides the matching `pnpm-workspace.yaml` setting, and the exclude flag may be repeated to build a list [#13929](https://github.com/pnpm/pnpm/issues/13929).

- An unreadable `node_modules/.modules.yaml` no longer makes `pnpm install` delete `node_modules` and relink every package on each run. The unparsable state file is now reported as an error instead [#14062](https://github.com/pnpm/pnpm/issues/14062).

- `pnpm outdated` and `pnpm update --interactive` now leave out the dependencies listed in `updateConfig.ignoreDependencies`, instead of reporting them and offering them for update.

- Fixed `pnpm outdated` and `pnpm update --interactive` offering versions blocked by `minimumReleaseAge` [pnpm/pnpm#14004](https://github.com/pnpm/pnpm/issues/14004).

- `pnpm pack` writes tar entries in the POSIX ustar header form npm uses — `ustar\0` magic and the explicit `0` regular-file typeflag — instead of the GNU form with a NUL typeflag, which strict tar readers such as publint mistake for the end-of-archive marker [#13924](https://github.com/pnpm/pnpm/issues/13924).

- Fixed `--config.ignore-scripts=true` not being honored by CLI commands such as `pnpm pack` [#13986](https://github.com/pnpm/pnpm/issues/13986).

- `pnpm install <pkg>` now adds the package, the same as `pnpm add <pkg>` and matching the JavaScript CLI. It previously ended in a usage error: `pnpm i valibot` printed `error: unexpected argument 'valibot' found` instead of saving the dependency [#13886](https://github.com/pnpm/pnpm/issues/13886).

- Fixed Plug'n'Play projects to preload `.pnp.cjs` for dependency and project lifecycle scripts, `pnpm run`, and `pnpm exec`. The generated loader now also exposes the public Yarn PnP API surface.

- Workspace packages declared with a parent-relative pattern in `pnpm-workspace.yaml` (`../shared`, `../../docs/*`) are discovered again. They were dropped from the project list, so `pnpm list -r` and `--filter` did not see them and a frozen install of a lockfile that already held their importer entries failed with `ERR_PNPM_PACKAGE_MANAGER_UNSAFE_IMPORTER_PATH`.

- `pnpm pkg get` and `pnpm pkg set` now accept hyphens inside a dot-notation property path, so `pnpm pkg get dependencies.some-package-name` reads the key instead of failing with `ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH`. The bracketed and quoted forms already worked and are unchanged.

- A path named by the `pnpmfile` setting that is not on disk now fails with `ERR_PNPM_PNPMFILE_NOT_FOUND` and names the file, instead of surfacing as a generic pnpmfile execution failure. Discovery of the default `.pnpmfile.mjs` / `.pnpmfile.cjs` is unaffected: a project that ships neither still installs normally.

- `pnpm remove` now prunes undecided entries (`"set this to true or false"`) from `allowBuilds` in `pnpm-workspace.yaml` when `sharedWorkspaceLockfile: true` and the corresponding packages are removed [pnpm/pnpm#13892](https://github.com/pnpm/pnpm/issues/13892).

- Suggest `pnpm shim add <runtime>` after pinning a project runtime when no project-aware global shim is installed. Explicit project-aware shims now reject unrelated global bin conflicts and are restored after a matching global package is removed or replaced by a version that drops its bin.

- `pnpm -r update --latest --depth 0 <selector>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when no project in the workspace declares a matching dependency, instead of silently doing nothing.

- Fixed repeat installs paying for a full lockfile comparison forever after a modification-time collision. When a `package.json` was last modified inside the same clock tick that the install recorded as its validation baseline — a fast install, a checkout that copied files with identical timestamps, or any filesystem that keeps only whole-second modification times — the manifest kept reading as possibly-modified, so every later `pnpm install` and `verify-deps-before-run` check re-compared the manifests against the lockfile instead of taking the fast path [#13907](https://github.com/pnpm/pnpm/issues/13907).

- The Rust CLI now honors five settings it recognized but ignored: `updateNotifier`, `legacyDirFiltering`, `initAuthorName` / `initAuthorEmail` / `initAuthorUrl`, `initLicense`, and `initVersion`. `pnpm install` and `pnpm add` check once a day for a newer pnpm and print how to get it (turn it off with `updateNotifier: false`); a `{<dir>}` filter selector can go back to matching the subtree below the directory with `legacyDirFiltering: true`; and `pnpm init` writes the configured author, license, and version into the `package.json` it scaffolds. `PNPM_CONFIG_INIT_VERSION` is now read as well.

  `maxsockets`, npm's spelling of `maxSockets`, is no longer ignored: both spellings are read from `pnpm-workspace.yaml`, the global config file, the environment, and the command line, in that increasing order of precedence — a value passed on the command line now wins even when the two sides spelled the setting differently.

  A `lastUpdateCheck` timestamp dated in the future — after a clock change, a restored snapshot, or a hand-edited state file — no longer silences the update check until that time comes around.

  `legacyDirFiltering` no longer reaches the workspace-root selectors pnpm generates for itself: the `!{<workspace-root>}` exclusion a recursive `run` / `exec` / `add` / `test` appends, and the `{<workspace-root>}` inclusion `--workspace-root` appends. Read as subtree matches they named every project below the root, so a recursive command under the setting selected nothing at all, and `--workspace-root` pulled in every project below the root instead of the root alone [#14101](https://github.com/pnpm/pnpm/issues/14101).

- `pnpm sbom` now honours `--filter-prod`, the full `--filter` selector syntax (dependency queries such as `pkg...`, `{dir}` and glob paths, `[since]` change queries, exclusions), and `--workspace-root`. Selectors that match no project print `No projects matched the filters` and write no SBOM, and `--split` emits its per-project SBOMs in a stable order.

  The universal `--fail-if-no-match` flag is supported too: any filtered command whose selectors match no workspace project now exits with code 1 [#14064](https://github.com/pnpm/pnpm/issues/14064).

- `pnpm sbom` now fails with `ERR_PNPM_SBOM_MISSING_IMPORTERS` when `pnpm-lock.yaml` has no entry for a selected project, instead of writing an SBOM that under-reports that project's dependencies. Previously this crashed with `Cannot read properties of undefined (reading 'devDependencies')`.

- `pnpm self-update` now rewrites a simple `devEngines.packageManager.version` range (`^`/`~`) to the newly installed version, keeping the operator — matching how `pnpm update` and `pnpm runtime set` rewrite ranges. Complex ranges such as `>=8.0.0` that the new version satisfies are still left unchanged [#13935](https://github.com/pnpm/pnpm/issues/13935).

- `pnpm update` now preserves the existing range operator when updating a prerelease dependency. See pnpm/pnpm#7002.

- `pnpm update <name>@<version>` now fails with `ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP` when the package is not a direct dependency of any selected project, instead of quietly updating it to whatever a fresh install would resolve. There is nowhere to record the version in that case, so the request cannot be honored, and the error points at the `overrides` entry that does pin a transitive dependency. Ranges and tags are unaffected, and a package that any selected project declares directly still takes its version as before.

- `trustPolicy: no-downgrade` no longer aborts the install with `ERR_PNPM_MISSING_TIME` on registries that serve no per-version `time` field when `minimumReleaseAgeIgnoreMissingTime` is set. The trust check reads the same publish dates the `minimumReleaseAge` check does, so it now honors the same opt-in and skips the affected package with a warning [#12446](https://github.com/pnpm/pnpm/issues/12446).

  `minimumReleaseAgeIgnoreMissingTime` no longer lets a lockfile entry the registry does not list pass the `minimumReleaseAge` check during lockfile verification. The opt-in covers a registry that cannot date its releases; a packument that does date every version it lists is saying it never published this one, which stays a hard failure.

  The missing-`time` warning now names the check it is reporting on, so a package whose `minimumReleaseAge` and `trustPolicy` checks are both skipped warns about both instead of only the first.

- `pnpm update <pkg>@<tag>` now saves the version the dist tag resolved to in `package.json`, keeping the range operator the dependency already declared, instead of saving the tag itself. A dependency declared through a `catalog:` reference, a `workspace:` or `npm:` alias, or a path or git specifier keeps its declaration, and one that already tracks a dist tag records the tag asked for [#14092](https://github.com/pnpm/pnpm/issues/14092).

- `pnpm update <pkg>@<version>` now updates only the selected packages and leaves unrelated dependencies unchanged. A selector that renames the package it installs — `pnpm update <alias>@npm:<pkg>@<version>` or the `jsr:` equivalent — now targets the package the alias installs rather than the alias.

- `pnpm version <bump>` with `--dry-run` no longer edits `package.json` files. It now only reports the bumps it would make, and skips the working tree check, the version lifecycle scripts, the commit, and the tag [`pnpm/pnpm#13953`](https://github.com/pnpm/pnpm/issues/13953).

- On Windows, `pnpm store path` now returns a conventional drive path without the `\\?\` verbatim prefix when the project and pnpm home are on different drives [#13987](https://github.com/pnpm/pnpm/issues/13987).
