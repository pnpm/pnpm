## 12.3.0

### Minor Changes

- Every context-aware global command (`node`, `deno`, `bun`, and the shims created with `pnpm shim add`) is now a native executable on every platform, so environment variables whose names are not valid shell identifiers reach these commands. On Windows, `<name>.exe` replaces the `.cmd` and `.ps1` shims for them. Shims written by earlier pnpm 12 releases are migrated on the next global install or self-update.

- `pnpm remove` and `pnpm update` now accept `--trust-lockfile`, `--no-trust-lockfile`, `--trust-policy`, `--trust-policy-exclude` and `--trust-policy-ignore-after`, the same flags `pnpm install` and `pnpm add` take, so the supply-chain settings can be overridden for a single run. `pnpm remove` verifies the lockfile against the active policies the way `pnpm install` does, and `--trust-lockfile` skips that pass for every entry, not only the package being removed.

  `pnpm` now also honors `--config.trust-lockfile=<value>`, and accepts the bare `--trust-lockfile` / `--no-trust-lockfile` spelling on the commands that previously took the setting from the config file alone.

### Patch Changes

- `pnpm add <local directory>`, `pnpm add <local tarball>`, `pnpm add file:<path>` and `pnpm add <tarball URL>` work again: a specifier given without a `<name>@` prefix is no longer read as a registry package name and rejected with `ERR_PNPM_PACKAGE_MANAGER_ADD_RESOLVE_LATEST` [#14437](https://github.com/pnpm/pnpm/issues/14437).

- Fixed `pnpm deploy --legacy` ignoring `allowUnusedPatches` supplied through `--config.allow-unused-patches` or the `PNPM_CONFIG_ALLOW_UNUSED_PATCHES` environment variable [pnpm/pnpm#14450](https://github.com/pnpm/pnpm/issues/14450).

- Fixed `pnpm install --lockfile-only` writing a lockfile that referenced a missing peer-suffixed snapshot when an npm-aliased dependency took part in a cyclic peer dependency graph. The following `pnpm install --frozen-lockfile` failed with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` [#14449](https://github.com/pnpm/pnpm/issues/14449).

- `pnpm config` now accepts `-g`/`--global`, `--location`, and `--json` before its subcommand [pnpm/pnpm#14421](https://github.com/pnpm/pnpm/issues/14421).

- `pnpm dedupe` now converges in one pass when it re-resolves a lockfile created by pnpm 11, so a second run no longer changes the lockfile [#14455](https://github.com/pnpm/pnpm/issues/14455).

- Fixed detached child processes being terminated on Windows when another program launches `pnpm` directly, without a shell, as `nr` from `@antfu/ni` does [#14447](https://github.com/pnpm/pnpm/issues/14447).

- Fixed `pnpm docs <package>@<version>` ignoring the requested version. It now opens the selected version's homepage and reports a missing version instead of opening the package-level homepage [pnpm/pnpm#14428](https://github.com/pnpm/pnpm/issues/14428).

- Speed up installs in large workspaces by reading and parsing `pnpm-lock.yaml` on a background thread while workspace projects are being discovered, whenever the run is certain to need it (`--frozen-lockfile`, `--force`, or no state from a previous install on disk) [#14352](https://github.com/pnpm/pnpm/issues/14352).

- Fixed filtered and recursive `pnpm run` and `pnpm exec` hanging when a script reads from the terminal. A script the workspace never runs alongside another one — a single `--filter`ed project, `--workspace-concurrency=1`, a dependency chain, or a task declaring `concurrency: 1` — now stays in the terminal's foreground process group, so interactive prompts work again [#14397](https://github.com/pnpm/pnpm/issues/14397).

- Fixed false unmet peer errors for auto-installed peers in linked workspace packages.

- Fixed npm global installs on Windows so the PowerShell shims invoke `pnpm.exe`.

- Fixed `pnpm with current <command>` when global options precede it, such as `pnpm --workspace-root with current --version` [pnpm/pnpm#14413](https://github.com/pnpm/pnpm/issues/14413).

  A short-option cluster that mixes a global flag with an option owned by the command, such as `pnpm -ro dist pack-app`, is now parsed like the same options written after the command.

  An option written before the command name is now reported as an unknown option unless that command accepts it, instead of being taken for the command to run — `pnpm -P exec echo` and `pnpm -z exec echo` fail the way `pnpm --tag next exec echo` does.

- Apply pure insertions in zero-context patches at the correct line instead of one line early.

- Improved peer dependency resolution performance when many packages reuse the same peer ranges.

- `pnpm outdated` and `pnpm update` now follow local actions and reusable workflows referenced with GitHub's self-repository syntax (`uses: $/.github/actions/setup`) when looking for outdated GitHub Actions, the same way they follow `./` references.

- The `pnpm install --help` descriptions of `--prod` and `--dev` no longer claim that the flags take precedence over `NODE_ENV`. pnpm does not read `NODE_ENV` when selecting which dependency groups to install [#14445](https://github.com/pnpm/pnpm/issues/14445).

- Sped up installs in large workspaces: the fast lockfile-update check no longer compares every project against every lockfile entry (or copies the whole lockfile before discovering a change needs the resolver), project ordering uses faster hashing, and the version-preference table builds in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).

- Speed up dependency resolution in large workspaces: workspace `link:` targets are now re-anchored between the lockfile root and each consuming project with lightweight relative-path math instead of rebuilding and comparing absolute paths on every dependency edge [#14352](https://github.com/pnpm/pnpm/issues/14352).

- On Linux, pnpm now resolves registry hostnames through the system resolver (`getaddrinfo`), as it already does on macOS and Windows and as pnpm 11 did. Previously, an `/etc/resolv.conf` containing an option the bundled pure-Rust resolver did not recognize, such as `options no_tld_query`, made pnpm ignore the configured nameservers and silently query Google's public DNS instead [#14469](https://github.com/pnpm/pnpm/issues/14469).

- Speed up dependency resolution in large workspaces: the resolver's per-dependency cache keys now compare paths by their raw bytes instead of walking them component by component, and the importer-wide part of the shared workspace-resolution key is built once per project instead of once per dependency edge [#14352](https://github.com/pnpm/pnpm/issues/14352).

- `catalogMode` and `--save-catalog` no longer move a local path, tarball, or `workspace:<path>` specifier into a catalog — such a path is resolved against the project that declares it, so a shared catalog entry cannot mean the same directory for every project referencing it [#14437](https://github.com/pnpm/pnpm/issues/14437).

- Sped up installs in large workspaces: the workspace dependency graph is now built and searched for cycles once per run instead of twice, and its edges resolve in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).

- Speed up writing `pnpm-lock.yaml` in large workspaces: the entries of the big lockfile sections (`importers`, `packages`, `snapshots`) are now key-sorted and rendered to YAML in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).

- Fixed non-frozen installs through a pnpr server failing instead of regenerating a conflicted lockfile.

- `pnpm update --interactive` renders its checklist the way pnpm 11 does: group headings and column headers are separators the cursor skips instead of checkboxes that select nothing, the columns of one group line up with the next, `a` toggles all and `i` inverts the selection, and the confirmed selection is echoed as a list of package names [#14423](https://github.com/pnpm/pnpm/issues/14423).

- Fixed `pnpm config` commands targeting global configuration to skip project package manager version switching, allowing registry authentication to be configured before pnpm downloads a project-pinned version [pnpm/pnpm#14463](https://github.com/pnpm/pnpm/issues/14463).

- Fixed pnpm retaining the surrounding quotes in `.npmrc` values, including auth tokens expanded from environment variables. This restores authentication with registries configured using `:_authToken="${TOKEN}"` [pnpm/pnpm#14427](https://github.com/pnpm/pnpm/issues/14427).

- Fetch and tarball errors no longer print the secrets of the URL they name — inline `user:pass@` credentials, and the query string or fragment of a signed URL — so a failed install or `pnpm add <url>` cannot leak them into terminal scrollback or CI logs.

- When `dist-tags.latest` names a version whose manifest pnpm cannot read, the error now names that version and the field it could not decode, instead of reporting the tag as empty.

- Retry transient Windows file-lock errors, including sharing violations, while linking dependencies with the default (isolated) `nodeLinker`. This fixes [pnpm/pnpm#14407](https://github.com/pnpm/pnpm/issues/14407).

- Fixed `pnpm run`, `pnpm exec`, `pnpm rebuild`, and the script shortcuts (`pnpm test`, `pnpm start`, `pnpm stop`, `pnpm restart`, `pnpm <script>`) not loading the pnpmfile, so an `updateConfig` hook never applied to them [#14433](https://github.com/pnpm/pnpm/issues/14433). A hook's settings — `extraEnv` and `extraBinPaths` among them — now reach the scripts and commands these spawn, as they do on pnpm 11.

- The `pnpm` executable of the npm package now works when the package was installed without running its install scripts, as under `--ignore-scripts` or the default build-script block of pnpm and Bun [#14346](https://github.com/pnpm/pnpm/issues/14346). In that case it runs through Node.js and, in a terminal, says how to switch to the native binary.

- Sped up installs in large workspaces: the resolver now shares the already-parsed `pnpm-lock.yaml` instead of deep-copying it before every fresh resolution [#14352](https://github.com/pnpm/pnpm/issues/14352).

- `minimumReleaseAgeStrict` now defaults to `true` when `minimumReleaseAge` is explicitly configured — through `pnpm-workspace.yaml`, the global `config.yaml`, a `PNPM_CONFIG_*` variable, or a CLI flag — as documented. Previously an explicit cutoff was treated as non-strict, so immature versions were silently added to `minimumReleaseAgeExclude` instead of being gated with a prompt [#14409](https://github.com/pnpm/pnpm/issues/14409). The built-in 1440-minute default stays non-strict.

- Preserve environment variables whose names are not valid shell identifiers when launching Node.js installed by `pnpm runtime set node --global` on Unix [pnpm/pnpm#14417](https://github.com/pnpm/pnpm/issues/14417).

- Fixed `pnpm repo` and `pnpm docs` failing to open the Windows browser from WSL [pnpm/pnpm#14467](https://github.com/pnpm/pnpm/issues/14467).

- `pnpm link`, `pnpm outdated`, and `pnpm import` now apply pnpmfile `updateConfig` hooks before resolving dependencies.

- Fixed standalone installations to preserve the bundled `node-gyp` files used to build native dependencies.

- Fixed resolution against registries whose version manifests carry `_npmUser`, `dist.attestations`, `dist.unpackedSize`, `dist.fileCount`, or `peerDependenciesMeta` in a shape npm does not use. Such a version was skipped as though it had never been published, so `pnpm add` could fail with "no version found for the latest tag" even though the registry served it.

- `pnpm unpublish` now completes the two-factor authentication a registry asks for instead of failing with `ERR_PNPM_UNAUTHORIZED` while logged in: a 401 that is an OTP challenge starts the web-based authentication flow (or prompts for a classic one-time password), and the obtained password is reused by every request of the run [#14464](https://github.com/pnpm/pnpm/issues/14464).

- On Windows, pnpm now resolves host names through the system resolver instead of its own DNS client. The built-in client bound a UDP socket for every lookup, which made Windows Defender Firewall ask to allow `pnpm.exe` again after every `pnpm self-update` [#14405](https://github.com/pnpm/pnpm/issues/14405).
