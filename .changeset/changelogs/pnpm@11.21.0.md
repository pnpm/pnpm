## 11.21.0

### Minor Changes

- Added interactive group selection to `pnpm update --global --interactive`.

- Running `pnpm setup`, `pnpm self-update`, or a command that modifies the global installation (such as `pnpm add --global`) through `sudo` now prints a warning. pnpm keeps global packages and configuration in the invoking user's home directory, so running these commands as root silently operates on the root user's home directory instead of yours. They will fail with `ERR_PNPM_SUDO_NOT_SUPPORTED` in pnpm v12. Read-only global commands (such as `pnpm bin --global`) are unaffected.

### Patch Changes

- Fixed pnpm failing to start under asynchronous Node.js module loaders when no `.pnpmfile.mjs` exists [pnpm/pnpm#11701](https://github.com/pnpm/pnpm/issues/11701).

- Fixed `minimumReleaseAge` fallback for custom dist-tags so the selected version does not exceed the registry’s original tag target.

- Removing a dependency from `package.json` and reinstalling no longer re-resolves the dependency graph. The importer's entry is dropped from `pnpm-lock.yaml`, anything it made unreachable is pruned, and a catalog entry that loses its last referent is removed — all without registry access. Installs still fall back to a full resolution when a package that stays resolves a peer dependency through the removed one, since that would change the surviving package's entry rather than only prune.

- Changing a catalog entry to a different exact version no longer re-resolves the dependency graph. The package is replaced in `pnpm-lock.yaml` directly, reusing the same check the `pnpm.overrides` fast path applies: every locked dependency of the package must still satisfy the new version's manifest. Installs fall back to a full resolution when anything other than the catalog reaches the package — an importer that depends on it directly, or another package that depends on it — since the graph would then need both versions.

- Fixed a CI regression where `github:owner/repo` dependencies (and other shorthand Git specifiers) would fail to install with `Permission denied (publickey)` on CI runners that lack SSH keys. The Git resolver no longer records an SSH URL unless the user explicitly wrote one (e.g. `git+ssh://` or `git@host:...`):

  - The repository visibility probe (an HTTP HEAD request) now retries transient failures such as `429 Too Many Requests`, so host throttling of CI runners is no longer mistaken for a private repository.
  - For non-SSH specifiers, anonymous HTTPS `git ls-remote` access is now tried before SSH, so a public repository whose visibility probe fails still resolves to a portable HTTPS URL instead of an SSH URL that only works where SSH keys are configured.
  - When every probe fails, the resolver falls back to HTTPS for shorthand and HTTPS-style specifiers, and only guesses SSH when the user explicitly provided an SSH URL.
  - A repository that could not be confirmed public is no longer resolved to the host's anonymous archive URL (e.g. `codeload.github.com`, which would fail to download for a private repository); it stays a regular `git` resolution so installs can use ambient Git credentials such as credential helpers and tokens.

  Note that a private repository that is reachable both over authenticated HTTPS and over SSH now resolves to its HTTPS URL, where previous versions recorded the SSH URL.

  Fixes [pnpm/pnpm#13276](https://github.com/pnpm/pnpm/issues/13276).

  <!-- cspell:ignore publickey -->

- `ng build` and `nuxt build` now work under the global virtual store: pnpm's built-in compatibility extensions add the `tslib` dependency that `@angular/build` uses without declaring and the `unplugin` dependency that `@nuxt/vite-builder` v4 uses without declaring.

- Fixed `link:` dependencies under `enableGlobalVirtualStore` so linked children are materialized and slots remain isolated by their resolved link targets.

- An install that skips resolution because `pnpm-lock.yaml` is already up to date now reacts fully to packages the lockfile removed — for example after pulling a lockfile in which a dependency was deleted. The hoist layer is recomputed, so a package that became hoistable when a direct dependency was removed is hoisted, and `pendingBuilds` entries for removed packages are dropped instead of staying pending forever.

- The held-back-update warning printed by `pnpm update` no longer fires when `minimumReleaseAge` is the actual reason a newer version was not picked. The warning's baseline now applies the same maturity cutoff as the pick itself, so it no longer wrongly attributes the hold-back to "your manifests and already installed dependencies" or recommends an override that would defeat the age gate. See pnpm/pnpm#13071.

- Checking whether `ignoredOptionalDependencies` is up to date no longer reorders the configured patterns. The check sorted them in place, which could move an `!` exclusion ahead of the pattern it excludes from and flip which optional dependencies were ignored.

- Changing `autoInstallPeers`, `dedupePeers`, `peersSuffixMaxLength`, `excludeLinksFromLockfile`, or `injectWorkspacePackages` no longer re-resolves the dependency graph when the lockfile proves the setting cannot affect it: no package or project declares a peer dependency for the peer settings, and no project depends on a directory or on another workspace project for the link and injection settings. The new setting is recorded in `pnpm-lock.yaml` and the install proceeds from the existing resolution. Every other case still falls back to a full resolution.

- Adding, editing, or removing an entry in `patchedDependencies` no longer re-resolves the dependency graph. Resolution never reads a patch — it only records the patch file's hash against the package it matches — so the install now rewrites the affected entries in `pnpm-lock.yaml` and materializes the patched package from the store instead. Installs still fall back to a full resolution when the patched package is reachable as a peer dependency, and when the new configuration would leave a patch unused while `allowUnusedPatches` is off, so `ERR_PNPM_UNUSED_PATCH` is still reported.

- Resolving a private git repository no longer blocks on an interactive credential prompt: `git ls-remote` now fails fast with an authentication error when git has no credentials for the repository [#13522](https://github.com/pnpm/pnpm/issues/13522).

- Lockfile verification now honors offline mode by using cached registry metadata instead of reaching the registry. When the required metadata is not available locally, verification reports the same `ERR_PNPM_NO_OFFLINE_META` condition used by offline resolution.

- POSIX shell shims now follow symbolic links before computing `basedir`, preventing execution failures when a shim is invoked via an external symlink on `PATH` [#13405](https://github.com/pnpm/pnpm/issues/13405).

- The automatic `packageManager` version switch works again on registries whose tarball URLs point at a different host than the registry itself (load-balanced feed proxies, Artifactory-style mirrors). Package-manager entries are now always recorded with integrity-only resolutions — the download URL is derived from the trusted bootstrap registry instead — and entries persisted in an invalid shape by an earlier pnpm are discarded and re-resolved instead of failing every command [#13619](https://github.com/pnpm/pnpm/issues/13619).

- Registries that serve no npm signature metadata (private mirrors and feed proxies commonly strip `dist.signatures`) no longer break the automatic `packageManager` version switch and `pnpm self-update` [#13147](https://github.com/pnpm/pnpm/issues/13147). When the configured registry cannot provide a verifiable signature, pnpm now fetches the signature from `registry.npmjs.org` and verifies it against the same embedded npm keys over the installed integrity — which proves exactly the same thing. If no signature can be obtained from either source (for example, both are unreachable, or the registry publishes only a `shasum`), pnpm proceeds with a warning instead of failing, but only when the packages resolve through a registry configured in the user's own (non-project) configuration; the download stays pinned by the lockfile integrity, and a signature that exists but does not validate still fails the switch.

- `pnpm fetch`, and any install run with `virtualStoreOnly`, no longer writes a `.pnp.cjs` loader under `nodeLinker: pnp`. These installs populate the virtual store without linking the project, so the loader would have claimed the project resolves out of a store it was never linked into. The importer links and `node_modules/.package-map.json` were already skipped; the PnP loader now follows the same rule.

- Prevent pnpm from removing project files when `modulesDir` resolves to the project root.

- Speed up installs after adding `ignoredOptionalDependencies` patterns by removing newly ignored optional dependencies and pruning packages that are no longer reachable without resolving the dependency graph again.

- When a failed install re-copies a bin script from the store, rerunning `pnpm install` now reapplies the executable bit to the bin instead of leaving it non-executable [#12742](https://github.com/pnpm/pnpm/issues/12742).

- `pnpm root -g` and `pnpm bin -g` now print warnings to stderr instead of stdout, so their stdout stays a clean, machine-readable path. Previously, running either command with `--global` in a project that pins a package manager (e.g. via the `packageManager` field) printed a warning like `[WARN] Using --global skips the package manager check for this project` ahead of the path, breaking programs that capture the output as a path [#13672](https://github.com/pnpm/pnpm/issues/13672).

  In pnpm 12, `pnpm root -g` and `pnpm prefix -g` are now supported (they previously failed with `ERR_PNPM_CLI_ROOT_GLOBAL_UNSUPPORTED` / `ERR_PNPM_CLI_PREFIX_GLOBAL_UNSUPPORTED`), and the reporter output of `dlx`, `create`, `config`, `sbom`, `with`, `store`, `prefix`, `root`, and `bin` goes to stderr, matching pnpm 11.

- `pnpm setup` no longer makes Node.js print a `MODULE_TYPELESS_PACKAGE_JSON` warning about `dist/worker.js` on every command. The `package.json` it writes next to a standalone executable now declares `"type": "module"`.

- `pnpm update` without saving no longer records a version that the manifest's range excludes. The kept range stays authoritative: a requested version outside it is skipped with a warning, and a requested range, a dist tag, or `--latest` resolves within it instead of past it. Previously each of these could write a lockfile entry that contradicted its own specifier, which the next `pnpm install --frozen-lockfile` rejected with `ERR_PNPM_OUTDATED_LOCKFILE` [#12764](https://github.com/pnpm/pnpm/issues/12764).

- `pnpm version -r --json` now outputs `[]` instead of human-readable text when no pending changes exist [`pnpm/pnpm#13217`](https://github.com/pnpm/pnpm/issues/13217).
