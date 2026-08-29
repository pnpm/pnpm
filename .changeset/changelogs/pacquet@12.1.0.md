## 12.1.0

### Minor Changes

- `pnpm login` and `pnpm adduser` now record the granted token in the global `config.yaml`, under the `_auth` setting, with `--scope`'s scope routed to that registry under `registries`. `pnpm logout` removes it from there, and still from an `auth.ini` an earlier version wrote. Tokens already in `auth.ini` keep working.

- A `scope` set in a project's `pnpm-workspace.yaml` is now ignored, with a warning naming where to set it instead. `pnpm login` records the scope as a `@scope:registry` route in the machine-global `auth.ini`, which outranks `~/.npmrc` in every project — so a repository-committed file could redirect a scope such as `@acme` for all of a user's other projects after one routine login. Use `--scope`, the `PNPM_CONFIG_SCOPE` environment variable, or the global config file instead [#13557](https://github.com/pnpm/pnpm/issues/13557).

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

- Persist completed recursive tasks so `--resume-from` skips exactly the work that passed during a matching interrupted or failed `pnpm -r run` / `pnpm -r exec` invocation. When no compatible state exists, pnpm retains its graph-based resume behavior.

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- Added per-task concurrency limits to workspace task orchestration. Set `tasks.<name>.concurrency` in `pnpm-workspace.yaml` to limit how many instances of that task may run across workspace projects at once:

  ```yaml
  tasks:
    build:
      concurrency: 2
  ```

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

- Added macOS and Windows x64 and arm64 support to remote shared build artifacts [pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771).

- Generalized the experimental shared-artifact protocol so candidates and signed payloads identify a discriminated subject. Dependency side effects use package and source-integrity subjects, while workspace tasks use project and task subjects.

  This changes shared-artifact request bodies and signed payloads. A pnpr server and its clients have to be on matching versions.

### Patch Changes

- An `_auth` entry in the global config file no longer decides which registry packages come from when something else says. A `registry` or `registries` declared in `pnpm-workspace.yaml` or the global config now wins over the route inferred from a stored credential, which still applies where nothing else declares one. The `pnpm_config__auth` environment variable is unchanged: it stays the way to point a CI runner at a mandated proxy, and still overrides what a repository declares.

- Fixed `pnpm deploy --legacy` to exclude dependencies that are only reachable from unselected workspace projects after `pnpm fetch`.

- Fixed dependency-verification install logs corrupting `pnpm exec` output and ignoring `--silent` [pnpm/pnpm#14197](https://github.com/pnpm/pnpm/issues/14197).

- `pnpm clean` / `pnpm purge` run from a workspace subdirectory now remove each project's own `node_modules` instead of emptying the workspace root's for every project [#14239](https://github.com/pnpm/pnpm/issues/14239). A custom `modulesDir` is resolved against each project directory too.

- `pnpm dlx <pkg>@catalog:` now resolves the specifier through the calling workspace's catalogs instead of failing with `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC` [#14294](https://github.com/pnpm/pnpm/issues/14294).

- Fixed `pnpm doctor` reporting a version that does not match `pnpm --version` [pnpm/pnpm#14225](https://github.com/pnpm/pnpm/issues/14225).

- Pacquet now strips exactly one leading path component from `./`-prefixed tarball entries, matching pnpm and npm's tar extraction semantics and keeping shared store keys consistent.

- Installs whose lockfile carries platform or engine constraints are up to ~150 ms faster when resolution runs: the `node --version` probe behind the installability checks now starts before the lockfile is parsed and finishes while dependencies resolve, instead of running afterwards.

- Treat empty scripts selected by a regular expression as missing before running dependent tasks.

- Filter hidden scripts matched by a regular expression during recursive runs when a visible script also matches.

- Fixed `.mjs` pnpmfile hooks failing to load on Windows, including hooks supplied by config dependencies [pnpm/pnpm#14301](https://github.com/pnpm/pnpm/issues/14301).

- Fixed automatically switched pnpm versions forcing all descendant pnpm processes to use the same version [pnpm/pnpm#14309](https://github.com/pnpm/pnpm/issues/14309).

- Fixed `pnpm deploy --prod` failing when an excluded dev dependency was also declared as an optional peer dependency [pnpm/pnpm#14302](https://github.com/pnpm/pnpm/issues/14302).

- Fixed `pnpm pack` to respect the `files` field when deciding whether to include root-level changelog, history, and notice files.

- `pnpm update -g` no longer downgrades a global package. `--latest` resolves the `latest` dist-tag, which can point at an older release than the one installed — after `pnpm add -g <pkg>@next`, for instance [#14270](https://github.com/pnpm/pnpm/issues/14270).

  `pnpm update -g` also no longer changes the pnpm version. pnpm's own global install belongs to `pnpm self-update` [#14270](https://github.com/pnpm/pnpm/issues/14270).

- When multiple versions of the same package expose the same binary, pnpm now links the binary from the highest version [#14249](https://github.com/pnpm/pnpm/issues/14249).

- `pnpm update` no longer replaces the specifier a project declares for a dependency that is also listed in `overrides`. A `catalog:` reference stays a `catalog:` reference, and a declared range stays as written, instead of being rewritten to the version the override resolved to [#12115](https://github.com/pnpm/pnpm/issues/12115).

- `pnpm update` no longer moves the range a project declares for a dependency that `overrides` also lists, even when the override repeats that range verbatim. Previously the updated `package.json` disagreed with the lockfile, so the next `pnpm install --frozen-lockfile` failed with a specifier mismatch [#14224](https://github.com/pnpm/pnpm/issues/14224).

- Allowed pnpm's shared-artifact client to connect to an artifact-only pnpr tier.

- Rebuilding `node_modules` from an up-to-date lockfile is up to ~200 ms faster: the `node --version` probe that installability checks and store keying need now runs concurrently with the store's warm-cache reads instead of before them.

- Remove the duplicate colon from the one-time password prompt.

- Print errors as JSON on stdout when `--json` is passed to `pnpm view` or its aliases (`info`, `show`, and `v`).

- Installs complete faster on workspaces with many projects: each project's `node_modules` is now linked concurrently.

- Fixed `patchedDependencies` matching for git-hosted dependencies during fresh and frozen installs [pnpm/pnpm#14273](https://github.com/pnpm/pnpm/issues/14273).

- `pnpm pm <command>` works again: the `pm` prefix, which forces pnpm's built-in command over a `package.json` script of the same name, is recognized instead of failing with `ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL` / `Command "pm" not found`. `pnpm pm clean` and `pnpm pm purge` now remove `node_modules` even when the project (or the workspace root) declares a `clean` / `purge` script [#14226](https://github.com/pnpm/pnpm/issues/14226).

- The settings that pnpm accepts as command-line flags are recognized again: `--package-import-method`, `--hoist-pattern`, `--public-hoist-pattern`, `--no-hoist`, `--global-dir`, `--virtual-store-dir`, `--modules-dir`, `--child-concurrency`, `--no-lockfile`, `--strict-peer-dependencies`, `--side-effects-cache`, `--side-effects-cache-readonly`, `--trust-policy`, `--trust-policy-exclude`, `--trust-policy-ignore-after`, and `--optimistic-repeat-install`. Each is accepted anywhere on the command line, spelled either `--setting=value` or `--setting value`, and overrides the same setting read from `pnpm-workspace.yaml` or `.npmrc` [#14281](https://github.com/pnpm/pnpm/issues/14281).

- `pnpm add`, `pnpm update`, and `pnpm remove` now save `package.json` before failing with `ERR_PNPM_IGNORED_BUILDS`. The dependency they were asked to change is already materialized by that point, so the manifest has to record it — otherwise the next install removes the packages again.

- The progress output no longer overwrites the lines above it once it grows taller than the terminal window [#14270](https://github.com/pnpm/pnpm/issues/14270).

- Restoring a dependency's build from the remote side-effects cache no longer downloads files the store already holds.

- Recognize `pnpm install --fix-lockfile`, including filtered installs, and regenerate broken lockfile metadata while preserving compatible locked versions [pnpm/pnpm#14250](https://github.com/pnpm/pnpm/issues/14250).

- Fixed intermittent `Access is denied` failures when concurrent global commands hand off the global bin lock on Windows.

- Fixed the `--shamefully-hoist` CLI option being rejected [pnpm/pnpm#14235](https://github.com/pnpm/pnpm/issues/14235).

- The environment variables for the remote side-effects cache are named for the setting they configure: `PNPM_SIDE_EFFECTS_CACHE_REMOTE_KEY_ID`, `..._BUILDER_ID`, `..._IMAGE_DIGEST`, `..._ARCHITECTURE_BASELINE`, `..._PRIVATE_KEY`, `..._BUILD_ENV`, `..._TRUSTED_KEYS` and `..._PUBLISH`. The `PNPM_REMOTE_SIDE_EFFECTS_CACHE_*` names keep working, and the new one wins when both are set.

- Installs that run no build scripts finish faster, especially in workspaces with many projects.

- A `devEngines.packageManager` range pin on pnpm is now recorded in `pnpm-lock.yaml`'s `packageManagerDependencies` when the running pnpm already satisfies it, using the running version and keeping the range as the recorded specifier. Previously only an exact pin — or a range resolved on the way through a version switch — reached the lockfile, so a range pin written by hand (or by any tool other than `pnpm add` / `pnpm self-update`) left the project without the shared resolution the pin exists to provide.

- Workspace installs are substantially faster (~0.7 s on a 60-project workspace): after hoisting, pnpm now shims only the bins of publicly hoisted workspace packages instead of re-walking every project's `node_modules` to rediscover bins that were already linked.

- Fixed a large install-time regression on macOS for installs that rebuild `node_modules` from a warm store [#14231](https://github.com/pnpm/pnpm/issues/14231). APFS serializes file-cloning and hard-linking syscalls volume-wide, so importing packages one file at a time from many threads was bounded by a per-volume ceiling and got slower the more CPU cores the machine had. On macOS, `pnpm install` now materializes each package once into the store's `links` directory (the same canonical slots `enableGlobalVirtualStore` uses) and copies it into `node_modules/.pnpm` with a single copy-on-write directory clone per package, replacing tens of thousands of per-file syscalls with one per package. Applies with the default `nodeLinker: isolated` when `enableGlobalVirtualStore` is off and `packageImportMethod` is `auto`, `clone`, or `clone-or-copy`; hoisted, global-virtual-store, and explicit `hardlink`/`copy` installs are unchanged.

- Stop in-flight recursive `run` and `exec` commands when bailing after the first failure.

- Warm installs that rebuild `node_modules` on macOS are about 10% faster: creating each package's virtual-store directory now issues fewer filesystem calls.

- An `_auth` credential in an `.npmrc` now authenticates even when its base64 is written without the trailing `=` padding (or with extra padding, or with whitespace inside it), instead of failing with a 401. An `_auth` that is not valid base64, or that carries no `:` between the username and the password, now fails with `ERR_PNPM_AUTH_INVALID_BASE64` / `ERR_PNPM_AUTH_MISSING_SEPARATOR` [#14257](https://github.com/pnpm/pnpm/issues/14257).

- Colored output is no longer printed as raw escape sequences in the Windows Command Prompt [#14292](https://github.com/pnpm/pnpm/issues/14292). Commands such as `pnpm list` now style their output there.
