## 11.26.0

### Minor Changes

- Added `pnpm change check`. It validates the committed package versions against the `versioning.epics` bands and the `versioning.fixed` groups in `pnpm-workspace.yaml` and lists every violation. It is meant to run in CI, because `pnpm version -r` only checks the packages it releases.

- `pnpm remove` and `pnpm update` now accept `--trust-lockfile`, `--no-trust-lockfile`, `--trust-policy`, `--trust-policy-exclude` and `--trust-policy-ignore-after`, the same flags `pnpm install` and `pnpm add` take, so the supply-chain settings can be overridden for a single run. `pnpm remove` verifies the lockfile against the active policies the way `pnpm install` does, and `--trust-lockfile` skips that pass for every entry, not only the package being removed.

  The Rust CLI now also honors `--config.trust-lockfile=<value>`, and accepts the bare `--trust-lockfile` / `--no-trust-lockfile` spelling on the commands that previously took the setting from the config file alone.

- Catalogs can now resolve workspace dependencies through the `workspace:` protocol.

### Patch Changes

- Build scripts can now be rejected before the package is installed [#14067](https://github.com/pnpm/pnpm/issues/14067):

  - `pnpm add --allow-build=!<pkg>` records `<pkg>: false` in `allowBuilds`. It used to write a `!<pkg>: true` entry that matched no package. On a global install the denial was dropped altogether, so the post-install prompt offered the build for approval.
  - `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` record their decision even when no packages are awaiting approval, and report a package that is not awaiting approval with a warning so a typo stays visible. Both cases used to fail the command with an error.

- Fixed `pnpm audit --fix` failing with `ERR_PNPM_INVALID_FIX_OPTION` when used without a value, including when another flag follows it, as in `pnpm audit --fix --json` [#13261](https://github.com/pnpm/pnpm/issues/13261). Fixed `pnpm audit --fix=override` ignoring the `saveExact` and `savePrefix` settings when writing vulnerability overrides [#11523](https://github.com/pnpm/pnpm/issues/11523).

- `pnpm audit` no longer counts advisories suppressed by `auditConfig` in its summary. The headline total and the `Severity:` breakdown now count only the advisories that survive the `ignoreGhsas` filter. Suppressed advisories get their own line, `2 ignored: 1 moderate | 1 critical`. A run whose advisories were all suppressed printed a red `1 vulnerabilities found` next to a zero exit code. It now reads `All found vulnerabilities were already reviewed and decided to be ignored` [#14535](https://github.com/pnpm/pnpm/issues/14535).

- Authenticate Node.js runtime downloads from `nodeDownloadMirrors` with URL-scoped npm registry credentials, including bearer tokens, basic auth, and `tokenHelper` [pnpm/pnpm#14334](https://github.com/pnpm/pnpm/issues/14334).

- Made downloaded runtimes available to dependency lifecycle scripts during installation.

- A relative `scriptShell` path in `pnpm-workspace.yaml` is now resolved against the workspace root, so scripts run from a nested workspace package find the shell [#14422](https://github.com/pnpm/pnpm/issues/14422). A bare command name such as `bash` is still looked up on `PATH`.

- `pnpm deploy` no longer requires `injectWorkspacePackages` to be enabled. A linked workspace dependency is rewritten to a `file:` dependency in the dedicated deploy lockfile, and the peer dependencies it declares are bound to the deployed graph's own resolution.

  When a peer resolves to more than one version in that graph the binding is ambiguous, and choosing between the candidates is exactly what injecting the package would have decided, so the deploy still fails — now with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER`, which names the package, the peer, and the competing versions, instead of refusing every non-injected workspace up front, and suggests pinning the peer to one version with an `overrides` entry as the way to keep deploying without injection [#9386](https://github.com/pnpm/pnpm/issues/9386).

- The JavaScript pnpm can again switch to the pnpm version a project pins in `packageManager` on hosts where the native pnpm build ships no binary, such as Alpine Linux with pnpm 10 or an Intel Mac with pnpm 11 [#13622](https://github.com/pnpm/pnpm/issues/13622).

  When the pnpm build being switched to is native and ships no binary for the host, pnpm now names the host target it lacks. pnpm reported that the binary was missing from `pnpm-lock.yaml`.

- Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` in a project with config dependencies when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it. pnpm no longer rewrites the lockfile when the recorded config dependencies are unchanged. Writing changed config dependencies through a symlinked lockfile is still refused. A lockfile that starts with a byte order mark now keeps its main document when its config dependencies are updated [#14372](https://github.com/pnpm/pnpm/issues/14372).

- Fixed global virtual store hashes for dependency cycles. Every package that transitively depends on an allowed build now includes the engine in its store path, independent of traversal order [pnpm/pnpm#14341](https://github.com/pnpm/pnpm/issues/14341).

- `pnpm outdated` and `pnpm update` now follow local actions and reusable workflows referenced with GitHub's self-repository syntax (`uses: $/.github/actions/setup`) when looking for outdated GitHub Actions, the same way they follow `./` references.

- `globalDir` and `globalBinDir` are honored wherever they are set, so `pnpm add -g` no longer fails with `ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH` after `pnpm config set -g global-bin-dir` [#14336](https://github.com/pnpm/pnpm/issues/14336). The global `config.yaml` is read again, `PNPM_CONFIG_GLOBAL_DIR` / `PNPM_CONFIG_GLOBAL_BIN_DIR` reach the directories derived from them, and a leading `~/` is expanded before that derivation. A project's `pnpm-workspace.yaml` still cannot set either key.

- Installation no longer fails with `Cannot convert undefined or null to object` when a linked local dependency provides a peer dependency that is also provided by one of its ancestors. This was reachable via `pnpm deploy --legacy`.

- `pnpm install` no longer lets a symlink swapped into the path of `pnpm-lock.yaml` during a config dependency update redirect the lockfile write to the symlink target [pnpm/pnpm#14322](https://github.com/pnpm/pnpm/issues/14322).

- `pnpm install --node-linker=hoisted` no longer downloads every optional dependency it reports as skipped when `node_modules` already exists. Those downloads also continued after pnpm printed `Done` [#14139](https://github.com/pnpm/pnpm/issues/14139).

- `pnpm import` now leaves the project-local lockfile unchanged when `lockfileDir` points to another directory. Failed imports restore the destination lockfile. Imports that use a branch lockfile leave the shared lockfile unchanged [#14563](https://github.com/pnpm/pnpm/issues/14563).

- Fixed concurrent installs sharing a store occasionally failing with an ENOENT error while importing a package file [#14353](https://github.com/pnpm/pnpm/issues/14353).

- Network and archive retry logs hide credentials and signed query parameters in request URLs.

- `catalogMode` and `--save-catalog` no longer move a local path, tarball, or `workspace:<path>` specifier into a catalog. Such a specifier is resolved against the project that declares it, so one catalog entry cannot mean the same directory for every project that references it [#14437](https://github.com/pnpm/pnpm/issues/14437).

- An auto-installed optional peer is now resolved to a version its declared peer range accepts, even when the workspace root depends on that package at a version outside the range. Previously the root's version was used and then reported as an unmet optional peer [#13867](https://github.com/pnpm/pnpm/issues/13867).

- `pnpm self-update`, `pnpm with`, and automatic package-manager version switching no longer wait through registry retry delays when a configured registry has no signatures and `registry.npmjs.org` is unavailable [#14483](https://github.com/pnpm/pnpm/issues/14483).

- Fixed `pnpm config` commands targeting global configuration to skip project package manager version switching, allowing registry authentication to be configured before pnpm downloads a project-pinned version [pnpm/pnpm#14463](https://github.com/pnpm/pnpm/issues/14463).

- Fetch and tarball errors no longer print the secrets of the URL they name. Inline `user:pass@` credentials and the query string or fragment of a signed URL are hidden, so a failed install or `pnpm add <url>` cannot leak them into terminal scrollback or CI logs.

- `pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs report an outdated lockfile until it is regenerated [pnpm/pnpm#14488](https://github.com/pnpm/pnpm/issues/14488).

- Fixed argument forwarding on Windows when `shellEmulator` is enabled. Paths ending in a backslash, line breaks, and literal shell expressions are preserved [pnpm/pnpm#14548](https://github.com/pnpm/pnpm/issues/14548).

- Fixed `--side-effects-cache`/`--no-side-effects-cache` and `PNPM_CONFIG_SIDE_EFFECTS_CACHE` discarding a remote side-effects cache declared under the object form of `sideEffectsCache` in `pnpm-workspace.yaml`. The boolean now switches only the local cache off or on, as it already does when a config file declares it.

- Fixed standalone installations to preserve the bundled `node-gyp` files used to build native dependencies.

- `pnpm unpublish` now completes the two-factor authentication a registry asks for instead of failing with `ERR_PNPM_UNAUTHORIZED` while logged in. A 401 that is an OTP challenge starts the web-based authentication flow, or prompts for a classic one-time password. The obtained password is reused by every request of the run [#14464](https://github.com/pnpm/pnpm/issues/14464).

- pnpm 12 now accepts the boolean settings as command-line flags on every command that takes them in pnpm 11, for example `pnpm install --unsafe-perm`, `pnpm add foo --offline`, and `pnpm install --dangerously-allow-all-builds`. pnpm 12 rejected them with `unexpected argument`, which failed every install on Vercel, whose build runs `pnpm install --unsafe-perm` [#14346](https://github.com/pnpm/pnpm/issues/14346).

  `pnpm remove` now accepts `--unsafe-perm`, the same flag `pnpm install`, `pnpm add`, and `pnpm update` take.
