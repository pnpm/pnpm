## 12.0.0-rc.1

### Major Changes

- Git dependencies on known hosts (GitHub, GitLab, Bitbucket) are now treated as identities rather than transport choices. Every representation of the same repository — `github:owner/repo`, `owner/repo`, `git+https://…`, `git+ssh://git@…` — resolves through the host's canonical HTTPS URL, and the lockfile never records an SSH URL for them. Repositories whose archive endpoint is anonymously reachable resolve to the host's archive (fast tarball download); all others resolve to a `git` clone of the canonical HTTPS URL, which every machine with access to the repository can fetch.

  To reach a private hosted repository over SSH, configure the machine (not the project) with git's own URL rewriting, for example:

  ```sh
  git config --global url."git@github.com:".insteadOf https://github.com/
  ```

  pnpm shells out to `git`, so the rewrite applies to all of pnpm's git operations automatically. URLs of unknown hosts (self-hosted servers) are unaffected and keep their exact URL, including SSH. URLs with embedded credentials are also kept verbatim and never resolve to a host archive.

  This removes the network probing that previously decided between HTTPS and SSH at resolution time, which could record a transport that only worked on the machine that happened to run the resolution (e.g. an SSH URL that broke CI runners without SSH keys).

### Minor Changes

- Added interactive group selection to `pnpm update --global --interactive`.

- `pnpm root -g` and `pnpm bin -g` now print warnings to stderr instead of stdout, so their stdout stays a clean, machine-readable path. Previously, running either command with `--global` in a project that pins a package manager (e.g. via the `packageManager` field) printed a warning like `[WARN] Using --global skips the package manager check for this project` ahead of the path, breaking programs that capture the output as a path [#13672](https://github.com/pnpm/pnpm/issues/13672).

  In pnpm 12, `pnpm root -g` and `pnpm prefix -g` are now supported (they previously failed with `ERR_PNPM_CLI_ROOT_GLOBAL_UNSUPPORTED` / `ERR_PNPM_CLI_PREFIX_GLOBAL_UNSUPPORTED`), and the reporter output of `dlx`, `create`, `config`, `sbom`, `with`, `store`, `prefix`, `root`, and `bin` goes to stderr, matching pnpm 11.

### Patch Changes

- Fixed `minimumReleaseAge` fallback for custom dist-tags so the selected version does not exceed the registry’s original tag target.

- Removing a dependency from `package.json` and reinstalling no longer re-resolves the dependency graph. The importer's entry is dropped from `pnpm-lock.yaml`, anything it made unreachable is pruned, and a catalog entry that loses its last referent is removed — all without registry access. Installs still fall back to a full resolution when a package that stays resolves a peer dependency through the removed one, since that would change the surviving package's entry rather than only prune.

- Dependencies declared with an empty version range (`"adler-32": ""`) install again instead of failing with `ERR_PNPM_NO_MATCHING_VERSION` [#13673](https://github.com/pnpm/pnpm/issues/13673). An omitted range means "any version", as it does in npm and pnpm v11, so packages that publish one — such as `js-xlsx`, `codepage`, and `ssf` — no longer need an `overrides` entry to install.

- Changing a catalog entry to a different exact version no longer re-resolves the dependency graph. The package is replaced in `pnpm-lock.yaml` directly, reusing the same check the `pnpm.overrides` fast path applies: every locked dependency of the package must still satisfy the new version's manifest. Installs fall back to a full resolution when anything other than the catalog reaches the package — an importer that depends on it directly, or another package that depends on it — since the graph would then need both versions.

- Fixed installs under `enableGlobalVirtualStore` failing with `failed to remove existing directory ... prior to swap: Directory not empty` (or `No such file or directory`) when peer variants of an injected `file:` dependency hash to the same slot. The link pass now materializes each unique slot directory once instead of racing one force-mode import per peer variant against the same path.

- The held-back-update warning printed by `pnpm update` no longer fires when `minimumReleaseAge` is the actual reason a newer version was not picked. The warning's baseline now applies the same maturity cutoff as the pick itself, so it no longer wrongly attributes the hold-back to "your manifests and already installed dependencies" or recommends an override that would defeat the age gate. See pnpm/pnpm#13071.

- Changing `autoInstallPeers`, `dedupePeers`, `peersSuffixMaxLength`, `excludeLinksFromLockfile`, or `injectWorkspacePackages` no longer re-resolves the dependency graph when the lockfile proves the setting cannot affect it: no package or project declares a peer dependency for the peer settings, and no project depends on a directory or on another workspace project for the link and injection settings. The new setting is recorded in `pnpm-lock.yaml` and the install proceeds from the existing resolution. Every other case still falls back to a full resolution.

- Adding, editing, or removing an entry in `patchedDependencies` no longer re-resolves the dependency graph. Resolution never reads a patch — it only records the patch file's hash against the package it matches — so the install now rewrites the affected entries in `pnpm-lock.yaml` and materializes the patched package from the store instead. Installs still fall back to a full resolution when the patched package is reachable as a peer dependency, and when the new configuration would leave a patch unused while `allowUnusedPatches` is off, so `ERR_PNPM_UNUSED_PATCH` is still reported.

- `pnpm install` again records immature versions picked under `minimumReleaseAge` (when `minimumReleaseAgeStrict` is off) in `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`, so a later frozen install of the same lockfile passes verification [#13687](https://github.com/pnpm/pnpm/issues/13687).

- Reduced peak install memory: cached registry metadata is now read on demand from the on-disk metadata cache instead of being held in memory for the whole resolution. Resolving a large peer-heavy graph (`@teambit/bit`) peaks at about 1.3 GB instead of 3.2 GB, and a full cold install of it stays under 2 GB [#13681](https://github.com/pnpm/pnpm/issues/13681).

- Lockfile verification now honors offline mode by using cached registry metadata instead of reaching the registry. When the required metadata is not available locally, verification reports the same `ERR_PNPM_NO_OFFLINE_META` condition used by offline resolution.

- POSIX shell shims now follow symbolic links before computing `basedir`, preventing execution failures when a shim is invoked via an external symlink on `PATH` [#13405](https://github.com/pnpm/pnpm/issues/13405).

- Speed up installs after adding `ignoredOptionalDependencies` patterns by removing newly ignored optional dependencies and pruning packages that are no longer reachable without resolving the dependency graph again.

- `pnpm self-update` no longer fails with `the installed pnpm wrapper is missing` when the global packages directory carries a `pnpm-workspace.yaml` of global settings (written there when a global install persists an `allowBuilds` decision). The engine install stays anchored to its own install directory instead of walking up and adopting that file as its workspace root. The `pnpm dlx` cache install gets the same anchoring, so a stray `pnpm-workspace.yaml` above the cache directory can no longer break it [#13697](https://github.com/pnpm/pnpm/issues/13697).

- Reduced peak memory usage and allocation churn during peer dependency resolution on workspaces with many peer-dependency issue occurrences [#13681](https://github.com/pnpm/pnpm/issues/13681).

- `pnpm update` without saving no longer records a version that the manifest's range excludes. The kept range stays authoritative: a requested version outside it is skipped with a warning, and a requested range, a dist tag, or `--latest` resolves within it instead of past it. Previously each of these could write a lockfile entry that contradicted its own specifier, which the next `pnpm install --frozen-lockfile` rejected with `ERR_PNPM_OUTDATED_LOCKFILE` [#12764](https://github.com/pnpm/pnpm/issues/12764).

- `pnpm version -r --json` now outputs `[]` instead of human-readable text when no pending changes exist [`pnpm/pnpm#13217`](https://github.com/pnpm/pnpm/issues/13217).

- On Windows, installation no longer fails with "A required privilege is not held by the client. (os error 1314)" when symlink creation requires elevation (e.g. Developer Mode is off) — pnpm now falls back to NTFS junctions in that case. Additionally, `pnpm clean` and `pnpm deploy --force` no longer fail with "Access is denied. (os error 5)" when removing the package links inside `node_modules` [#13694](https://github.com/pnpm/pnpm/issues/13694).
