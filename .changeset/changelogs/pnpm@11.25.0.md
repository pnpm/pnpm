## 11.25.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Added macOS and Windows x64 and arm64 support to remote shared build artifacts [pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771).

- Added the `audit.ignorePrune` setting. When set to `true`, `pnpm audit --fix` removes ignored GHSA entries that no longer appear in the audit report.

- Generalized the experimental shared-artifact protocol so candidates and signed payloads identify a discriminated subject. Dependency side effects use package and source-integrity subjects, while workspace tasks use project and task subjects.

  This changes shared-artifact request bodies and signed payloads. A pnpr server and its clients have to be on matching versions.

- `pnpm init` now pins the latest pnpm version, instead of the version of pnpm that ran the command. A project scaffolded by an outdated pnpm therefore no longer inherits that staleness through its own `devEngines.packageManager` / `packageManager` pin [#7490](https://github.com/pnpm/pnpm/issues/7490).

  The version is read from the `latest` tag on the package-manager registries. When that lookup cannot answer — no network, an unreachable or slow registry, `offline`, or a `latest` that the `minimumReleaseAge` / `trustPolicy` settings reject — `pnpm init` pins the running version as before, and never fails or hangs on the lookup. A `latest` that is older than the running pnpm is never pinned either.

- A `scope` set in a project's `pnpm-workspace.yaml` is now ignored, with a warning naming where to set it instead. `pnpm login` records the scope as a `@scope:registry` route in the machine-global `auth.ini`, which outranks `~/.npmrc` in every project — so a repository-committed file could redirect a scope such as `@acme` for all of a user's other projects after one routine login. Use `--scope`, the `PNPM_CONFIG_SCOPE` environment variable, or the global config file instead [#13557](https://github.com/pnpm/pnpm/issues/13557).

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

- Persist completed recursive tasks so `--resume-from` skips exactly the work that passed during a matching interrupted or failed `pnpm -r run` / `pnpm -r exec` invocation. When no compatible state exists, pnpm retains its graph-based resume behavior.

- Allowed `pnpm update --patches` to refresh registry revisions through a configured pnpr server while retaining locked package versions.

- Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- `pnpm stage approve` now approves several staged packages at once. Run it without a stage id to pick from the staged versions interactively, or pass a list of stage ids. The whole batch is approved with a single one-time password, and pnpm asks for a new one only once the registry stops accepting it. Inside a workspace, the selected packages are approved in dependency order, and a package whose workspace dependency could not be approved is skipped instead of being published against a dependency that never reached the registry.

- Added per-task concurrency limits to workspace task orchestration. Set `tasks.<name>.concurrency` in `pnpm-workspace.yaml` to limit how many instances of that task may run across workspace projects at once:

  ```yaml
  tasks:
    build:
      concurrency: 2
  ```

- Added support for registry replacement tarballs using standard integrity values, explicit revision fields, registry routing from the `registries` setting, non-redirecting integrity-addressed URLs, canonical safe-integer revision numbers, and pnpr proxying for immutable upstream revision artifacts.

- `sideEffectsCache` now declares the whole of how a package's build output is reused — whether one is restored, whether one is saved, and the remote tier that shares it between machines:

  ```yaml
  sideEffectsCache:
    read: true
    write: true
    remote:
      org: acme
      packages: ['native-addon']
  ```

  `sideEffectsCache: true`, `sideEffectsCacheReadonly`, `remoteSideEffectsCache`, and its `organization` field all keep working. Where a field is set under both spellings the one above wins; where it is set under only one, it is kept.

  Two behaviors change, both bringing this CLI in line with what the Rust one already did: `sideEffectsCacheReadonly: true` now blocks writing to the cache, and setting it alongside `sideEffectsCache: false` gives a read-only view rather than switching the cache off entirely. A cache can also be declared write-only now, to populate one the run does not read.

- Workspace task orchestration ([pnpm/rfcs#23](https://github.com/pnpm/rfcs/pull/23)). `pnpm -r run` and `pnpm -r exec` now schedule per task instead of in topological chunks: a task starts as soon as the tasks it depends on have finished, so a project no longer waits for unrelated projects that happen to share its chunk.

  A new `tasks` section in `pnpm-workspace.yaml` declares what a task depends on, using the `^` convention:

  ```yaml
  tasks:
    build:
      dependsOn: ['^build']
    test:
      dependsOn: ['build']
    lint: {}
  ```

  `^name` means the named task in each of the project's workspace dependencies; a bare `name` means the task in the same project; an entry with no `dependsOn` declares an empty dependency list. A task with no entry behaves as `dependsOn: ['^<its own name>']`, which is exactly what the previous chunked ordering implied — an unconfigured workspace gets the scheduler improvement and nothing else changes meaning. A project without the script is reported skipped and passes its edges through to its own dependencies, so a scriptless package does not sever a chain.

  Also part of this change:

  - A dependency cycle among the tasks of a run is now an error naming the participating tasks (`ERR_PNPM_TASK_CYCLE`) instead of silently running in an arbitrary order. Setting `ignoreWorkspaceCycles: true` downgrades the error to a warning: the cycle's tasks run in an arbitrary order relative to each other.
  - `--resume-from` now skips exactly the transitive dependencies of the anchor package; work unrelated to the anchor still runs.
  - Under `--no-bail`, tasks whose dependencies failed are reported as skipped, not failed, and do not add to the exit code.
  - With `--bail` (the default), the first failure still ends the run at once and nothing new is dispatched — including scripts already queued behind the concurrency limit.
  - `pnpm -r run --dry-run <script>` prints the task graph that would execute without running anything (including skipping the `verifyDepsBeforeRun` check); `--json` emits the tasks and their resolved dependency edges.
  - Output is inherited rather than piped only when at most one script can ever be in flight (`--workspace-concurrency=1`, or the graph forces the scripts to run one after another).

### Patch Changes

- An `_auth` entry in the global config file no longer decides which registry packages come from when something else says. A `registry` or `registries` declared in `pnpm-workspace.yaml` or the global config now wins over the route inferred from a stored credential, which still applies where nothing else declares one. The `pnpm_config__auth` environment variable is unchanged: it stays the way to point a CI runner at a mandated proxy, and still overrides what a repository declares.

- Prevent installs through a symlinked `node_modules` directory from rewriting the target checkout [pnpm/pnpm#14286](https://github.com/pnpm/pnpm/issues/14286).

- Treat empty scripts selected by a regular expression as missing before running dependent tasks.

- The options type of the `fetch` command now declares `allowBuilds`, a setting its handler already forwarded to the installer. Type-level only — what `pnpm fetch` does is unchanged.

- Filter hidden scripts matched by a regular expression during recursive runs when a visible script also matches.

- Fixed automatically switched pnpm versions forcing all descendant pnpm processes to use the same version [pnpm/pnpm#14309](https://github.com/pnpm/pnpm/issues/14309).

- Fixed `ERR_PNPM_UNUSED_PATCH` validation during incremental installs [pnpm/pnpm#13692](https://github.com/pnpm/pnpm/issues/13692).

- Fixed `pnpm deploy --prod` failing when an excluded dev dependency was also declared as an optional peer dependency [pnpm/pnpm#14302](https://github.com/pnpm/pnpm/issues/14302).

- `pnpm update -g` no longer downgrades a global package. `--latest` resolves the `latest` dist-tag, which can point at an older release than the one installed — after `pnpm add -g <pkg>@next`, for instance [#14270](https://github.com/pnpm/pnpm/issues/14270).

  `pnpm update -g` also no longer changes the pnpm version. pnpm's own global install belongs to `pnpm self-update` [#14270](https://github.com/pnpm/pnpm/issues/14270).

- Copying a built package to its other hoisted locations no longer replaces the destination directory. With `nodeLinker: hoisted`, that replacement deleted the dependencies nested inside the destination's `node_modules`, and made concurrent copies of the same build chunk fail with `ERR_PNPM_ENOENT: no such file or directory, rename '.../node_modules/_tmp_...'` [#12880](https://github.com/pnpm/pnpm/issues/12880).

- `pnpm update` no longer replaces the specifier a project declares for a dependency that is also listed in `overrides`. A `catalog:` reference stays a `catalog:` reference, and a declared range stays as written, instead of being rewritten to the version the override resolved to [#12115](https://github.com/pnpm/pnpm/issues/12115).

- `pnpm update` no longer moves the range a project declares for a dependency that `overrides` also lists, even when the override repeats that range verbatim. Previously the updated `package.json` disagreed with the lockfile, so the next `pnpm install --frozen-lockfile` failed with a specifier mismatch [#14224](https://github.com/pnpm/pnpm/issues/14224).

- Make `pnpm add --lockfile-only` skip dependency linking [pnpm/pnpm#14286](https://github.com/pnpm/pnpm/issues/14286).

- `--production` is accepted again as an alias of `--prod` on `install`, `fetch`, `prune`, `update`, `list`, `why`, and `sbom`, and the install that `verifyDepsBeforeRun` reproduces is now spelled with `--prod`. `pnpm run` no longer aborts with "unexpected argument '--production' found" after a production-only install [#14147](https://github.com/pnpm/pnpm/issues/14147).

- The progress output no longer overwrites the lines above it once it grows taller than the terminal window [#14270](https://github.com/pnpm/pnpm/issues/14270).

- Restoring a dependency's build from the remote side-effects cache no longer downloads files the store already holds.

- Forward `patchedDependencies` hashes and `packageExtensions` to pnpr so server-side resolution preserves patches and package extensions in the lockfile and installed packages.

- Published the workspace task graph and scheduler as `@pnpm/workspace.task-scheduler` so other workspace commands can use the same dependency-aware scheduling as recursive run and exec.

- The environment variables for the remote side-effects cache are named for the setting they configure: `PNPM_SIDE_EFFECTS_CACHE_REMOTE_KEY_ID`, `..._BUILDER_ID`, `..._IMAGE_DIGEST`, `..._ARCHITECTURE_BASELINE`, `..._PRIVATE_KEY`, `..._BUILD_ENV`, `..._TRUSTED_KEYS` and `..._PUBLISH`. The `PNPM_REMOTE_SIDE_EFFECTS_CACHE_*` names keep working, and the new one wins when both are set.

- A `devEngines.packageManager` range pin on pnpm is now recorded in `pnpm-lock.yaml`'s `packageManagerDependencies` when the running pnpm already satisfies it, using the running version and keeping the range as the recorded specifier. Previously only an exact pin — or a range resolved on the way through a version switch — reached the lockfile, so a range pin written by hand (or by any tool other than `pnpm add` / `pnpm self-update`) left the project without the shared resolution the pin exists to provide.

- Fixed recursive `run` cleanup on Windows when a lifecycle script fails while another script's process tree is still running.

- The update notification now suggests `pnpm self-update` when `PNPM_HOME` manages the pnpm in use, and the [standalone install script](https://pnpm.io/installation) otherwise — under Corepack, or when another package manager installed pnpm. `pnpm self-update` under Corepack names the standalone install script too.

- Enforce `allowBuilds` when a prepared git dependency is reused from the shared store, and use the lockfile's canonical git resolution ID in approval suggestions.

- Topologically sorting workspace projects now runs in linear time, fixing installs and lockfile updates that stalled for seconds on workspaces with thousands of projects forming deep dependency chains [#14149](https://github.com/pnpm/pnpm/issues/14149), [#14151](https://github.com/pnpm/pnpm/issues/14151).
