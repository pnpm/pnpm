## 12.0.0

### Major Changes

- Git dependencies on known hosts (GitHub, GitLab, Bitbucket) are now treated as identities rather than transport choices. Every representation of the same repository — `github:owner/repo`, `owner/repo`, `git+https://…`, `git+ssh://git@…` — resolves through the host's canonical HTTPS URL, and the lockfile never records an SSH URL for them. Repositories whose archive endpoint is anonymously reachable resolve to the host's archive (fast tarball download); all others resolve to a `git` clone of the canonical HTTPS URL, which every machine with access to the repository can fetch.

  To reach a private hosted repository over SSH, configure the machine (not the project) with git's own URL rewriting, for example:

  ```sh
  git config --global url."git@github.com:".insteadOf https://github.com/
  ```

  pnpm shells out to `git`, so the rewrite applies to all of pnpm's git operations automatically. URLs of unknown hosts (self-hosted servers) are unaffected and keep their exact URL, including SSH. URLs with embedded credentials are also kept verbatim and never resolve to a host archive.

  This removes the network probing that previously decided between HTTPS and SSH at resolution time, which could record a transport that only worked on the machine that happened to run the resolution (e.g. an SSH URL that broke CI runners without SSH keys).

- A project's `pnpm-workspace.yaml` may no longer carry a setting pnpm does not recognize. Such a setting used to be ignored in silence — a misspelled `minimumReleaseAge` dropped the policy it was meant to set, and nothing said so. Now it is reported, suggesting the closest real setting name when the key looks like a typo, and it fails the command with `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS` when the project pins a pnpm version the running pnpm satisfies: with the pin honored, the setting cannot be meant for a different pnpm version, so it is a mistake to fix rather than a key to ignore. Everywhere else it is a warning, so a project that has yet to be cleaned up keeps working.

  The `pnpm config` subcommands never fail on such a setting, so a broken file can still be inspected and repaired, and `pnpm config get <key>` prints the value with no warnings at all. Keys the global config file cannot set are likewise split between workspace-only settings (still directed to `pnpm-workspace.yaml`) and settings unknown to this version.

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Added the `audit.ignorePrune` setting. When set to `true`, `pnpm audit --fix` removes ignored GHSA entries that no longer appear in the audit report.

- `pnpm init` now pins the latest pnpm version, instead of the version of pnpm that ran the command. A project scaffolded by an outdated pnpm therefore no longer inherits that staleness through its own `devEngines.packageManager` / `packageManager` pin [#7490](https://github.com/pnpm/pnpm/issues/7490).

  The version is read from the `latest` tag on the package-manager registries. When that lookup cannot answer — no network, an unreachable or slow registry, `offline`, or a `latest` that the `minimumReleaseAge` / `trustPolicy` settings reject — `pnpm init` pins the running version as before, and never fails or hangs on the lookup. A `latest` that is older than the running pnpm is never pinned either.

- Allowed `pnpm update --patches` to refresh registry revisions through a configured pnpr server while retaining locked package versions.

- Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.

- Added support for registry replacement tarballs using standard integrity values, explicit revision fields, registry routing from the `registries` setting, non-redirecting integrity-addressed URLs, canonical safe-integer revision numbers, and pnpr proxying for immutable upstream revision artifacts.

- `packageImportMethod: auto` now tries hardlinks before cloning on Linux. A reflink materializes a new inode and copies extent bookkeeping inside the filesystem's metadata trees, where a hardlink is one directory entry — on btrfs this roughly halves the time an install spends materializing `node_modules` from a warm store. ext4 installs are unchanged (cloning was never supported there, so `auto` already hardlinked), and macOS keeps clone-first, where APFS `clonefile` is the platform's cheap primitive. Cloning remains the fallback when the store refuses hardlinks, and remains available explicitly via `packageImportMethod: clone`.

  This ships with pnpm 12 only: pnpm 11's importer deliberately keeps clone-first, since changing what the default materializes on disk is not a point-release change.

- Added global build approvals [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- Added `pnpm cache path`, which prints the directory pnpm uses for its metadata cache. CI setups can use it to cache that directory — including the lockfile verification log, which lets a job skip re-checking an unchanged lockfile against the configured supply-chain policies.

- Added recursive global outdated checks [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- **Breaking change.** Dependency cycles are now broken canonically during peer resolution: the members of each cycle are ordered by package id, and the edges that close a cycle are always cut at the same place, no matter where the installation walks into the cycle from. Previously the cut depended on the walk path, so installing the same dependencies could produce different lockfiles depending on importer order or resolution order [#13846](https://github.com/pnpm/pnpm/issues/13846), and a peer-resolution verdict computed for one occurrence of a cyclic package could be wrongly reused at another [#13865](https://github.com/pnpm/pnpm/issues/13865).

  With canonical cycle breaking the lockfile is a pure function of the dependency graph: repeated installs, reordered importers, and reordered dependencies all produce byte-identical lockfiles. Peer dependencies of packages inside a cycle keep nearest-wins resolution along the canonical order, and a dependency edge that closes a cycle references an occurrence of its target resolved at the importer level. On large cycle-heavy workspaces peer resolution is 2–3× faster, uses about 25% less memory, and produces a substantially smaller lockfile (fewer redundant peer variants).

  Existing lockfiles keep working: headless (`--frozen-lockfile`) installs consume them unchanged, and installs that skip resolution leave them untouched. The first install that actually re-resolves (for example after a dependency change) re-keys walk-order-dependent peer variants of cyclic packages once.

- Completed pnpm runtime installation parity for Node.js, Deno, and Bun, including runtime failure policy, target architecture selection, and dependency runtime engines. Runtime failure overrides now preserve explicit runtime dependencies without matching engine entries.

- `pnpm config get` and `pnpm config list` now show the settings pnpm acts on under their documented names:

  - `registries` shows the registries pnpm resolves from, merged across every source (`.npmrc`, `pnpm-workspace.yaml`, the global config, CLI flags), in the shape the setting is written in: keyed by registry URL, with the default registry declared as the bare `@` scope. Built-in routes are included — the `@jsr` scope and the `npmjs` and `gh` prefixes — unless pointed elsewhere. Previously `pnpm config get registries` printed `undefined`.
  - `update` and `audit` show the effective sections, whichever spelling set them. The deprecated internal spellings (`updateConfig`, `auditConfig`, `auditLevel`) are no longer listed.
  - `catalogs` shows the complete resolved catalog set — the singular `catalog` block is its `default` entry — whichever spelling declared it.
  - The `registry` and `@scope:registry` entries show the merged routes rather than raw `.npmrc` values, so they always agree with the `registries` view.

- Added support for configuring `stateDir` in the Rust pnpm CLI [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).

- `node_modules/.modules.yaml` no longer records the registries an install resolved from, and the recorded copy is dropped from the file on the first install that rewrites it.

  It dated from the lockfile format that spelled a dependency's path relative to its registry, where reading an installed tree meant knowing the registries it was installed with. Dependency paths have not carried a registry for several major versions, and the recorded copy outlived its use: `pnpm list`, `pnpm why`, and single-project installs preferred it over the project's own configuration, so a project whose registry had changed since its last install was still read through the old one.

  They now use the configured registries, like every other command already did.

- Added `versioning.epics` to `pnpm-workspace.yaml`. An epic ties a group of member packages to a lead package, constraining every member's major version to a band derived from the lead's major: while the lead is on major `M`, members live in `M*100 … M*100+99`. Members move independently inside the band (patch, minor, and a `major` intent that stays in-band); a bump that would carry a member past the band ceiling is rejected until the lead advances its own major. When a release plan takes the lead to a new stable major, every member re-bases to the band floor in the same plan. Membership is matched with pnpm's package selectors — name globs, `./`-prefixed directory globs, and `!`-prefixed negations.

- Made peer resolution significantly faster in large multi-importer workspaces (a 114-importer workspace's resolution dropped from ~77s to ~36s): importers whose hoist rounds converged no longer re-walk their dependency forest every round, later rounds walk only newly added direct dependencies, ownership handovers with an unchanged peer context no longer invalidate shared walk caches, and the resolver's internal hash maps use a faster hash. Peer dependencies provided by multiple candidate versions may resolve to a different (still range-valid) provider than before, which can shift some peer-variant suffixes in `pnpm-lock.yaml` once.

- The first release of a package now publishes the version written in its manifest verbatim, instead of bumping off it. `pnpm version -r` and `pnpm change status` check the registry for each release's current version; when that version is not yet published, the package debuts at it and its pending changesets apply only from the next release. A newly added package seeded at `1100.0.0` with a `minor` changeset is therefore published as `1100.0.0` rather than skipping straight to `1100.1.0`.

- Added interactive group selection to `pnpm update --global --interactive`.

- Added bounded workspace concurrency for recursive run and exec commands [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- Added support for alias-less Git dependency adds, preserved locked Git commits during unrelated dependency changes, and reported Git package versions in install logs.

- Added a new setting, `update.githubActionsServer`, for specifying the base URL of the GitHub server that hosts the repositories of the GitHub Actions referenced by the workflow files (for example, a GitHub Enterprise Server). When the setting is not defined, the URL is read from the `GITHUB_SERVER_URL` environment variable, falling back to `https://github.com`. The URL must use the `https://` or `http://` protocol [#13220](https://github.com/pnpm/pnpm/issues/13220).

  `pnpm outdated` and `pnpm update` no longer fail when the refs of a GitHub Action's repository cannot be read (for example, when the action's repository is private or hosted on a different GitHub server). Such actions are now skipped with a warning.

  Setting `update.githubActions` to `false` now makes `pnpm outdated` and the interactive `pnpm update` skip GitHub Actions dependencies.

- Globally installed bins can now follow the project you run them in. The new `globalShims` setting is a record of package names to policies that selects which globally installed packages get project-aware shims; it defaults to `{ node: true, deno: true, bun: true }` and merges key-wise, so `globalShims: { bun: false }` switches one default off and `globalShims: { typescript: true }` adds another package. With the default, a project that pins Node.js through `devEngines.runtime` or `engines.runtime` gets the pinned stable release — authenticated against the Node.js release-team signatures — downloaded on first use and run whenever you type `node` inside the project, with no shell hooks. Candidates that are not signature-verified (Deno, Bun, Node.js prereleases, and ordinary package bins you enable) ask "Do you trust this project?" once per candidate and remember the answer machine-locally; the record values name the policy per package: `"auto"` (or its shorthand `true`) defers to artifact authentication, `"always"` switches without ever asking (useful in CI), and `"prompt"` always asks, even for authenticated candidates. Set `globalShims: false` to disable the feature, or `PNPM_SHIM_BYPASS=1` to bypass it for one invocation. On Windows, programs can keep spawning the global `node.exe` directly, without a shell.

- When `enableGlobalVirtualStore` is on, every process pnpm spawns for the project (`pnpm run`, `pnpm exec`, lifecycle scripts) now receives a `NODE_PATH` pointing at the project's hoisted `node_modules`, plus a `NODE_OPTIONS` `--import` flag that registers a resolve hook restoring `NODE_PATH` lookups for ESM imports. Dependencies that import undeclared ("phantom") packages keep resolving under the global virtual store — for both CommonJS and ESM — without installing the `@pnpm/plugin-esm-node-path` config dependency [pnpm/pnpm#9618](https://github.com/pnpm/pnpm/issues/9618). Tools run by `pnpm dlx` resolve such dependencies too: the JS CLI passes them the same environment, while the Rust CLI's dlx cache is self-contained, so its layout already exposes them.

- `pnpm list` and `pnpm why` are now feature complete and behaviorally identical to the TypeScript CLI. `pnpm list` gained `--only-projects`, `--find-by` (finders declared in `.pnpmfile.cjs`), search by version range (`pnpm ls "foo@^2"`), subtree deduplication with `[deduped]` markers, peer/skipped annotations, the package-count summary, `--long` manifest details, resolved tarball URLs and absolute paths in `--json`/`--parseable` output, and `--depth` support for globally installed packages. `pnpm why` gained `--json`, `--parseable`, `--long`, `--prod`/`--dev`/`--no-optional`, `--find-by`, workspace project names in the reverse tree, dependency-field annotations, `[circular]`/`[deduped]` markers, peer-variant hashes, and the `Found N versions` summary.

- `pnpm login` no longer requires an interactive terminal when the registry supports web-based login: without a TTY it prints the authentication URL (skipping the QR code and the "Press ENTER to open the URL in your browser" prompt) and polls the registry until the browser approval completes. Only the classic username/password login still fails with `ERR_PNPM_LOGIN_NON_INTERACTIVE` in a non-interactive terminal.

- Optional peer dependencies declared only via `peerDependenciesMeta` (for example `debug`'s `supports-color` peer) are now resolved from a satisfying version already present in the dependency graph, the same way explicitly declared optional peer dependencies are. Previously such peers were only resolved this way when the package's metadata was read back from the lockfile, so an unrelated dependency change could rewrite peer resolutions across the whole lockfile.

- Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

  Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.

- **Security fix.** Affects projects using `namedRegistries` on pnpm 11.1.0–11.19.x. It is **semi-breaking** for those projects — see "If you use named registries" below.

  The lockfile recorded no marker for which registry a package came from. Packages were keyed by `name@version` alone, and entry lookup went through `refToRelative(ref, name)`, so a dependency you declared against one registry could be satisfied by an entry that was actually resolved from another. When two registries served the same name and version, both collapsed onto a single `packages:` entry and whichever resolved first decided the tarball every consumer got.

  That is a package-substitution risk: a package you expect from your private registry could be installed from a different registry that publishes the same name and version, and the lockfile recorded nothing that would let you tell.

  Packages resolved from a named registry are now recorded under registry-qualified keys (`<name>@<registryName>:<version>`, e.g. `foo@work:1.0.0`), so each registry gets its own entry and the lockfile pins which one a dependency came from.

  The lockfile format version is unchanged. Registry-qualified keys appear only for packages resolved from a named registry, so a project that does not use `namedRegistries` sees no difference, and older pnpm versions keep reading the file.

  ### If you use named registries

  Your next non-frozen install re-keys those entries, which shows up as a lockfile diff. Commit it — that diff is the fix being applied. Review it: an entry that moves to a registry you did not expect is worth investigating.

  Everyone working on the project should be on this version or newer before you do. An older pnpm reads the re-keyed lockfile fine — frozen installs are unaffected — but it does not produce registry-qualified keys itself, so any install that updates the lockfile writes those entries back to the old shape, and the next install on a current pnpm re-qualifies them. The result is a lockfile that flips back and forth, and while it is in the old shape the project is exposed again. Because the lockfile format version is deliberately unchanged, pnpm cannot detect this and warn you about it.

  There is no setting to keep the old behavior: the old shape is the vulnerability.

  Tarball URLs that follow the standard registry layout are no longer written to the lockfile for named-registry packages; they are recomputed from the `namedRegistries` setting on demand.

  To use named registries, map your aliases in `pnpm-workspace.yaml`:

  ```yaml
  namedRegistries:
    work: https://npm.enterprise.example.com/
  ```

  ### New built-in `npmjs:` alias

  `npmjs:` now resolves to `https://registry.npmjs.org/` with no configuration, alongside the existing `gh:` alias for GitHub Packages. It pins a dependency to the public registry even when `registry` points elsewhere, such as an internal proxy:

  ```json
  { "dependencies": { "left-pad": "npmjs:^1.3.0" } }
  ```

  `npm:` cannot do this — it is the alias protocol (`npm:<name>@<range>`) and resolves through whatever `registry` points at.

  **If you mirror or proxy npmjs, point the alias at your mirror:**

  ```yaml
  namedRegistries:
    npmjs: https://npm.internal.example.com/
  ```

  Built-in registry URLs are also the prefixes a lockfile's recorded tarball URL is matched against when pnpm verifies a package. Without the override, an entry whose tarball URL is on `registry.npmjs.org` is verified against the public registry rather than your mirror. This only affects lockfiles that record such URLs — a canonical URL for your configured registry is omitted from the lockfile and unaffected — and only when a tarball-URL, `minimumReleaseAge`, or `trustPolicy` check runs. Overriding the alias is the same escape hatch GHES users already have for `gh`.

  Every alias the lockfile references must stay in `namedRegistries`: reading an entry whose alias is gone fails with `ERR_PNPM_MISSING_NAMED_REGISTRY` rather than silently falling back to the default registry, since that would fetch a different package. Renaming an alias re-resolves the packages that used it.

  Named registry aliases that shadow a reserved dependency specifier prefix (`file`, `link`, `workspace`, `runtime`, `npm`, `jsr`, ...) are now rejected with `ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME` instead of being silently shadowed by the corresponding resolver.

  `pnpm licenses` and `pnpm sbom` now keep the two artifacts apart as well: license records carry the registry alias, and SBOM components carry the purl `repository_url` qualifier.

- Added `projects[].dependencyManifest` to the `@pnpm/napi` install options: the manifest a workspace project exposes when it is resolved as a dependency of another importer (an injected instance). Hosts that pre-transform their importer manifests no longer need a `readPackage` hook to substitute the raw manifest, and per-manifest deletions are expressed through the existing `overrides` removal syntax (`"pkg": "-"`), so resolution can run without any JS round trips.

- `@pnpm/napi` gained reporter output, reverse dependency queries, and lockfile access.

  `install` and `rebuild` accept `options.reporter` and render pnpm's terminal output — progress line, packages-diff summary, lifecycle output, and the `Done in …` footer. Rendered output goes to stdout, or to an `onOutput` callback for a host that writes its own output through JavaScript. New reporting options: `hideLifecycleOutput`, `ignoredBuildsInstructionText`, and `hideLinkedPkgsDiff`.

  `getDependents` returns the reverse dependency trees behind `pnpm why`, annotated with the `package.json` fields named in `manifestFields`. `renderDependents` returns those trees rendered as tree, parseable, or JSON output.

  `readLockfile` and `writeLockfile` read and write `pnpm-lock.yaml` (or the current lockfile under the virtual store). `filterLockfileByImporters` returns a lockfile narrowed to what the named importers reach. `readModulesManifest` returns the `.modules.yaml` state of an installed `node_modules`.

  Top-level lockfile keys pnpm does not define are no longer dropped when a lockfile is loaded and saved, so state a tool records beside pnpm's own keys survives a rewrite.

- pnpm installs the other package managers now, not just itself: npm, Yarn Classic, Yarn Berry, Yarn 6 (`yarnpkg/zpm`), and Bun. Each is resolved and fetched through the trusted package-manager registries, and an npm-published one is verified against npm's signature for its exact version before it is executed.

  Three things use it:

  - A git-hosted dependency is prepared with the package manager it asks for. Its `packageManager` / `devEngines.packageManager` pin is honored, and a `yarn.lock` written by Yarn Classic no longer gets installed by Yarn Berry. pnpm provides that package manager when the dependency pinned a version, or when the host cannot satisfy what the dependency needs — so a repository built with Yarn now installs on a machine that has only pnpm, while a host that already has a suitable one keeps using its own.
  - `pnpm dlx` (`pnx`) runs one of them for a single command: `pnx yarn@4 install`, `pnx npm@11 ci`, `pnx bun@1.3.0 install`. Naming a package manager, or a runtime (`node`, `deno`, `bun`), there now provisions the real thing instead of installing the npm package that shares its name — unless the specifier locates a package rather than asking for a released version (`pnx yarn@npm:yarn@1.22.22`, `pnx yarn@yarnpkg/berry`), which installs what it names — `pnx yarn@4` was previously a missing version, since Yarn 4 is published as `@yarnpkg/cli-dist`, and `pnx node@22` now runs that Node.js release rather than a wrapper that downloads one. `--package` naming a package manager picks which of its commands to run, so `pnx --package npm@11 npx create-something` runs that npm's `npx`.
  - `pnpm shim add yarn` links a `yarn` command that runs whatever version the current project pins, and `pnpm shim rm` / `pnpm shim ls` manage those shims. It works for any package, not only package managers. Shims are never created as a side effect of `pnpm setup` or an install — a shim shadows the rest of your `PATH`, so pnpm only writes one when asked.

  Installing a package manager globally (`pnpm add -g yarn`) now makes it follow a project's pin too, the way a globally installed Node.js already follows `devEngines.runtime`: the pinned version runs where a project pins one, and the globally installed copy is the fallback everywhere else. An explicit `globalShims` entry, including `false`, is left as you set it.

  `pnpm add` follows the same rule about what a name means. `pnpm add -g yarn@4` installs Yarn Berry — it used to fail, because npm's `yarn` package stops at Classic — and `pnpm add -g node@22` / `pnpm add -g deno@2` install that Node.js or Deno release rather than a wrapper package that downloads one. In a project, naming a package manager records which one the project uses instead of installing it as a dependency, and naming a runtime records it under `engines.runtime` as `node@runtime:22` already did.

  The declaration goes where the package manager reads it. Yarn is started from a project pin by corepack, which reads only `packageManager` and only accepts an exact version there, so `pnpm add yarn@4` resolves the line and writes `"packageManager": "yarn@4.18.0"` — the same thing `corepack use yarn@4` writes, down to the `+sha512.…` integrity for the Yarn Classic line that corepack pins its tarball with. Every other package manager is recorded in `devEngines.packageManager`, which holds a range. Only one of the two fields is ever left behind: they declare the same thing, and corepack refuses to run a project whose declarations disagree.

  A JavaScript package manager on a machine without Node.js gets a managed LTS runtime to run on.

  What changes for a project coming from v11: `pnpm add yarn` records the project's package manager instead of installing the npm package that shares the name (that package is still reachable as `pnpm add yarn@npm:yarn@1.22.22`), `pnpm add -g yarn` installs the current Yarn line rather than Classic, `pnpm add -g node` / `pnpm add -g deno` and `pnx node` / `pnx deno` install a Node.js or Deno release rather than a wrapper package, and a globally installed package manager defers to a project's pin where there is one.

- Added support for the `cleanupUnusedCatalogs` setting: when enabled, `pnpm add`, `pnpm update`, and `pnpm remove` drop catalog entries from `pnpm-workspace.yaml` that no workspace project references.

- Added the `deprecate` and `undeprecate` commands for setting or removing the `deprecated` message on a package version (or semver range) in the registry, with support for `--registry` and `--otp`.

- Deprecated packages are reported during installation: a directly depended-on deprecated package gets an immediate warning, and deprecated subdependencies are summarized in a single `<N> deprecated subdependencies found` line. Versions matched by `pnpm.allowedDeprecatedVersions` are not warned about [#11633](https://github.com/pnpm/pnpm/issues/11633).

- The `enableModulesDir: false` setting is now honored: the install resolves and writes `pnpm-lock.yaml` but creates no `node_modules` directory (unless the global virtual store is enabled, in which case packages are still materialized into the store).

- Command shims now set `NODE_PATH` the way pnpm does: under the isolated `nodeLinker` with a hoist pattern, each shim lists the target package's own `node_modules` directories followed by the hidden hoisted modules directory (`node_modules/.pnpm/node_modules`). The new `extendNodePath: false` setting turns this off.

- `pnpm` now supports per-branch lockfiles in its Rust engine:

  - `gitBranchLockfile` gives each git branch its own `pnpm-lock.<branch>.yaml`, so two branches can hold different resolutions without conflicting on one file. A branch that has no lockfile yet installs against the shared `pnpm-lock.yaml`.
  - `mergeGitBranchLockfiles` (and the `--merge-git-branch-lockfiles` flag on `pnpm install`) folds every branch lockfile back into `pnpm-lock.yaml` and deletes them, which is what merging a branch into the mainline needs.
  - `mergeGitBranchLockfilesBranchPattern` (and `--merge-git-branch-lockfiles-branch-pattern`) names the branches that merge automatically, so a mainline branch does not have to pass the flag by hand [#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added `PNPM_CONFIG_VIRTUAL_STORE_ONLY` and `PNPM_CONFIG_ENABLE_MODULES_DIR` support to the Rust pnpm CLI.

- Added the `--force` flag to `pnpm install` and `pnpm add`: optional dependencies whose `cpu` / `os` / `libc` / `engines` don't match the host are installed instead of skipped, and a forced install relinks packages that an earlier install already materialized [#13142](https://github.com/pnpm/pnpm/issues/13142).

- The Rust engine now reads four more settings from `pnpm-workspace.yaml` and `PNPM_CONFIG_*`, instead of only accepting them as CLI flags:

  - `frozenLockfile` — `pnpm install` grows a `--no-frozen-lockfile` flag so the setting can be overridden in both directions. As in pnpm, it cannot be set in the global `config.yaml`.
  - `savePrefix` — the range operator `pnpm add` saves, still overridable with `--save-prefix` / `--save-exact`.
  - `savePeer` — `pnpm add` also records the new dependency in `peerDependencies`. `pnpm add --no-save-peer` overrides it back off.
  - `saveCatalogName` — the catalog `pnpm add` saves into.

- Implemented native `install-test` command.

- Added `pnpm licenses` command to the Rust pacquet port to list package licenses in a tabular or JSON format.

- Added support for the `lockfileDir` setting and its `--lockfile-dir <dir>` flag on `pnpm install`, `add`, `update`, and `remove`. `pnpm-lock.yaml`, the root `node_modules` holding the virtual store, and the config dependencies now live in the given directory, each project is recorded under its path relative to it, and every project keeps its own `node_modules` of symlinks — so several projects can share one lockfile [#12042](https://github.com/pnpm/pnpm/issues/12042).

- `pnpm version` now supports the npm-style bump forms: `pnpm version <major|minor|patch|premajor|preminor|prepatch|prerelease>` and `pnpm version <exact-version>` (also recursively with `-r`), with `--preid`, `--allow-same-version`, `--message`, `--no-git-tag-version`, `--no-commit-hooks`, `--sign-git-tag`, `--tag-version-prefix`, and `--json`. The bump runs the `preversion`/`version`/`postversion` lifecycle scripts and records the new version as a git commit and tag.

- Added the `owner` command (aliased as `owners`) for managing package owners on the registry, with the `ls` (default), `add`, and `rm` subcommands and support for `--registry` and `--otp`.

- Added support for the `preferSymlinkedExecutables` setting. On POSIX systems, `node_modules/.bin` entries are created as symlinks to the executable files instead of shell shims, and `NODE_PATH` pointing at the virtual store of the workspace root is exported to spawned scripts so they can resolve dependencies from the hoisted store. Like the TypeScript CLI, the setting turns on automatically when `nodeLinker` is set to `hoisted`.

- Added the six CLI flags the TypeScript pnpm CLI accepts but the Rust CLI did not [#14101](https://github.com/pnpm/pnpm/issues/14101):

  - `--stream` prints a recursive command's script output as it arrives, one line at a time, prefixed with the project it came from, instead of letting the scripts write to the terminal directly. `--parallel` implies it, as in pnpm.
  - `--aggregate-output` holds each script's streamed output until the script exits and then prints it as one block, so concurrent projects can't interleave.
  - `--reporter-hide-prefix` drops that project prefix from the scripts' own output lines. On a recursive `pnpm exec`, the opposite spelling `--no-reporter-hide-prefix` turns the prefixing on.
  - `--use-stderr` sends the reporter's output to stderr, leaving stdout for the command's own result.
  - `--ignore-workspace` runs the command as if the project were standalone: no workspace root is discovered, so `pnpm-workspace.yaml` contributes neither settings nor sibling projects, and a blocked dependency build is not scaffolded into its `allowBuilds`.
  - `--workspace-packages` overrides the `packages` patterns of `pnpm-workspace.yaml` for the run.

  The `stream`, `aggregateOutput`, `reporterHidePrefix`, `useStderr`, and `ignoreWorkspace` settings are now read from `pnpm-workspace.yaml`, the global `config.yaml`, and their `PNPM_CONFIG_*` environment variables too.

- Implemented native `recursive`, `multi`, and `m` commands in the Rust CLI.

- The Rust engine now supports the `saveWorkspaceProtocol` setting, so `pnpm add <pkg>@workspace:…` writes back the same specifier pnpm does. Under the default `rolling`, a request like `workspace:^1.2.3` is saved as `workspace:^` — a range with no version in it, so bumping the workspace package never has to touch its dependents' manifests. `saveWorkspaceProtocol: true` saves the workspace package's resolved version instead (`workspace:^2.5.0`), and `false` keeps the `workspace:` form only when it was asked for explicitly. Previously the specifier was written back exactly as typed.

- `sharedWorkspaceLockfile: false` is now supported by the install family [#12042](https://github.com/pnpm/pnpm/issues/12042): a workspace install runs one dedicated install per project, each with its own `pnpm-lock.yaml`, `node_modules`, and virtual store (a custom `virtualStoreDir` resolves per project), and `pnpm add` / `update` / `remove` in a project operate on that project's own lockfile. Recursive and filtered install-family commands still require a shared lockfile.

- Added support for the `shellEmulator` setting. With it enabled, the scripts `pnpm run` executes, a project's own lifecycle scripts, and dependencies' build scripts run in a built-in POSIX shell instead of the platform's (`sh -c`, or `cmd /d /s /c` on Windows), so scripts written for `sh` behave the same on every OS. `scriptShell` is not used while the emulator is on.

- Added the `star`, `unstar`, and `stars` commands. `star` and `unstar` mark or unmark a package as a favorite (falling back to editing the packument's `users` map on registries without the star endpoints), and `stars` lists the packages starred by the current or a specified user.

- The Rust engine now checks that a package read back from the store is the package it was recorded as. When the tarball's `package.json` names a different name or version than the store entry was keyed for — a broken lockfile, or a registry serving content that doesn't match its metadata — the install fails with `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE`. Set the new `strictStorePkgContentCheck` setting to `false` to downgrade the failure to a warning and install from the entry anyway [#12042](https://github.com/pnpm/pnpm/issues/12042).

- Added support for the `syncInjectedDepsAfterScripts` setting. It names the scripts after which every injected copy of the package that ran them is brought back in step with its source, so a build script no longer leaves the copies in the virtual store holding stale files.

- Added support for the `tokenHelper` auth setting, matching the TypeScript CLI. A `tokenHelper` configured in `~/.npmrc` or the global pnpm `auth.ini` names a command pacquet runs to obtain a registry token; the command runs lazily (only when a request to that registry is actually made), is given a 60-second time limit, and its output becomes the `Authorization` header. A `tokenHelper` in a workspace or project `.npmrc`, or supplied through a URL-scoped environment variable, is refused so a checked-in config can't run an arbitrary command.

- Added the `pnpm unpublish` command: remove a package from the registry entirely (requires `--force`), or remove the versions matching `<package>@<range>`, re-pointing `dist-tags` that referenced them and deleting the orphaned tarballs. Supports `--registry` and `--otp`.

- `pnpm update --workspace` is supported: dependencies that a workspace project publishes are re-pointed at the local copies through the `workspace:` protocol. The `saveWorkspaceProtocol` setting is honored — under its `rolling` default an entry becomes `workspace:*`, `workspace:^`, or `workspace:~` (whichever matches the range it already declared), so a sibling's next release does not invalidate it. Naming a dependency that is not in the workspace fails with `ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND`, and combining the flag with `--latest` fails with `ERR_PNPM_BAD_OPTIONS`.

  `pnpm update --depth <number>` is now applied per dependency instead of only distinguishing `0` from higher values: a dependency deeper than the given depth keeps its locked resolution, so `pnpm update --depth 0` updates direct dependencies only.

- Added the `virtualStoreOnly` setting, which populates the virtual store without any post-import linking — no importer symlinks, no `.bin` entries, no hoisting, and no project lifecycle scripts. Combining it with `enableModulesDir: false` fails with `ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR` unless `enableGlobalVirtualStore` is on, since the standard virtual store lives inside `node_modules`. A subsequent ordinary install completes the linking instead of treating the partially-populated directory as up-to-date. `enableModulesDir` is now read from `pnpm-workspace.yaml` as well.

- `pnpm` now supports three workspace settings in its Rust engine:

  - `includeWorkspaceRoot` (and the universal `--include-workspace-root` / `--no-include-workspace-root` flags) keeps the workspace root project in a recursive `run`, `exec`, `add`, or `test`, which otherwise leave it out.
  - `ignoreWorkspaceCycles` and `disallowWorkspaceCycles` control the report an install makes when workspace projects depend on each other in a cycle: it is a warning by default, an `ERR_PNPM_DISALLOW_WORKSPACE_CYCLES` error under `disallowWorkspaceCycles`, and silent under `ignoreWorkspaceCycles` [#12042](https://github.com/pnpm/pnpm/issues/12042).

- `peerDependencies` now accept dependency specifiers that carry a scheme — a named-registry spec (`<registry>:<version>`), an `npm:` alias, or a `file:`/git/URL spec — instead of rejecting them with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` [#13095](https://github.com/pnpm/pnpm/issues/13095). Such a peer is matched against the semver range carried by the specifier (`work:5.x.x` is checked as `5.x.x`, `npm:bar@^5` as `^5`), or against `*` when it carries no version, while the original specifier still selects the package to auto-install. Bare `name@version` values, which are almost always a mistake, are still rejected.

- A registry can now declare that its abbreviated metadata carries the `time` field, so `resolutionMode: time-based` reads the full metadata document only from the registries that need it:

  ```yaml
  resolutionMode: time-based
  registries:
    https://npm.internal.example/:
      supportsTimeField: true
  ```

  `registry.npmjs.org` omits `time` from abbreviated metadata, so a time-based resolution has to fall back to the much larger full document. That fallback used to be all-or-nothing: `registrySupportsTimeField` answered for every registry at once, so a project resolving from both the public registry and a Verdaccio instance either paid for full metadata everywhere or claimed a `time` field npmjs does not serve. The answer is now per registry, and `registrySupportsTimeField` remains the answer for every registry that does not declare one.

  The declaration is also sent to a pnpr server, which applies it to the resolution it runs on the client's behalf.

- Added `pnpm doctor`, which diagnoses the pnpm installation and the environment it runs in: the versions and install method, whether the global bin directory is on `PATH`, whether the store and cache are writable, which link strategies (reflink, hardlink, symlink) the store's filesystem supports, registry connectivity, and an offline `file:` install that exercises the resolve/store/link path end to end. Each check reports how to fix what it finds, and the command exits non-zero when any check fails.

  Use `--offline` to skip the checks that need network access, `--json` for machine-readable output, and `--benchmark` to time the filesystem and install checks.

- `pnpm setup` now appends `PNPM_HOME` and the global bin directory to the GitHub Actions environment files (`GITHUB_ENV` and `GITHUB_PATH`), so later steps in the same job can run `pnpm add --global` and other global commands [#9191](https://github.com/pnpm/pnpm/issues/9191).

- A pnpr resolve request now carries the client's registries the way the `registries` setting declares them — keyed by URL, with the scopes routed to each, the bare-specifier prefix each answers to, and each one's `serverType` — in place of the prefix map it used to send.

  The server routes them through the same inversion the config reader runs, so a pnpr-served install resolves a scoped dependency from the registry that scope is routed to, which it previously could not: only the default registry and the prefix-addressed ones reached the server. A declared `serverType` reaches it too, so the tarball URLs pnpr omits from the lockfile match the ones the client reconstructs.

  Built-in scope routes the project has not pointed elsewhere are not declared, so a pnpr server's allowlist is not asked about `npm.jsr.io` on requests that resolve no JSR package.

  A registry a request only declares is no longer refused up front for being off the server's allowlist — a client describes its whole configuration, including scopes a given resolve never reaches, so a stray `@scope:registry` in a developer's `~/.npmrc` no longer fails every install against a pnpr server that does not serve it. The boundary moves to the fetch itself: an origin the resolve does reach is refused before the request leaves the server, with the same message.

  This changes the resolve and verify-lockfile request bodies. A pnpr server and its clients have to be on matching versions; the protocol is still experimental and unversioned.

- A resolve request now carries the client's `resolutionMode`, so an install delegated to a pnpr server picks versions the way the client would. `time-based` and `lowest-direct` reached the server as nothing at all, leaving it on its `highest` default: the returned lockfile pinned the highest satisfying version of every dependency, and the setting appeared to be ignored.

  This adds a field to the resolve request body. A server older than its client ignores it and keeps resolving `highest`; the protocol is still experimental and unversioned.

- Added support for the remaining pnpm default settings, including recursive command controls, optional dependency selection, workspace-root checks, color modes, lockfile compatibility, and pack manifest options.

- Batch workspace publishing accepts a shared scope-specific credential, rejects mismatched credentials for a registry before publishing, and runs the `publish` and `postpublish` scripts after each completed registry group [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- Repeat installs now reconcile the existing `node_modules` the way the TypeScript CLI does: direct dependencies removed from the lockfile lose their links and bin shims, hoisted aliases of removed packages are unlinked so the next hoist pass can claim their slots, a hand-deleted package is detected and re-installed even when the lockfile is otherwise up to date, and `pnpm add` / `pnpm remove` fail with `ERR_PNPM_HOIST_PATTERN_DIFF`-family errors instead of silently recreating a modules directory whose layout settings drifted. Dev-only installs also no longer delete `node_modules/.pnpm/lock.yaml`.

- `pnpm install --ignore-scripts` now records the builds it skipped in `node_modules/.modules.yaml`'s `pendingBuilds`, and `pnpm rebuild --pending` runs them and clears the record instead of finding nothing to do. Both the dependencies whose build scripts were suppressed and the workspace projects whose own install scripts were suppressed are recorded and re-run, and an install that removes a package drops it from the list.

- Added recursive workspace support to `pnpm outdated`. `pnpm list` and `pnpm ll` now inspect all workspace projects by default, matching the TypeScript CLI.

- Made recursive `pnpm rebuild` honor workspace filters with shared and dedicated lockfiles.

- Made `pnpm why` and `pnpm peers` recursive by default in workspaces. Recursive peer checks now honor workspace filters, and recursive `why` can inspect the active project when a workspace uses dedicated lockfiles.

- Running `pnpm setup`, `pnpm self-update`, or a command that modifies the global installation (such as `pnpm add --global`) through `sudo` now fails with `ERR_PNPM_SUDO_NOT_SUPPORTED` instead of silently operating on the root user's home directory. pnpm keeps global packages and configuration in the invoking user's home directory, so these commands never need root permissions. Read-only global commands (such as `pnpm bin --global`) still work under sudo.

- `pnpm run "/^build:(backend|frontend)$/"` selects every script whose name matches the pattern, in single-project and recursive runs alike [#13322](https://github.com/pnpm/pnpm/issues/13322). Flags on the selector are rejected with `ERR_PNPM_UNSUPPORTED_SCRIPT_COMMAND_FORMAT`, as pnpm does.

- The `registries` setting now declares a registry once, keyed by its URL, with everything about that registry in the entry: how it lays out tarball URLs, the scopes routed to it, and the bare-specifier prefix it answers to.

  ```yaml
  registries:
    https://artifactory.example.com/artifactory/api/npm/npm-virtual/:
      serverType: artifactory
      scopes: ['@acme', '@acme-internal']
      prefix: work
  ```

  - **`serverType`** tells pnpm how the registry lays out its tarball URLs, which decides whether a URL can be omitted from `pnpm-lock.yaml`:
    - **undeclared** (the default) — strict. Only the exact canonical URL is treated as reconstructible.
    - **`npm`** — the registry behaves like `registry.npmjs.org`, which also serves a scoped package from its percent-encoded path. Declare this for a faithful mirror or caching proxy of the public registry so its tarball URLs can be omitted too.
    - **`artifactory`** — JFrog Artifactory repeats the scope in a scoped package's tarball filename (`@acme/widget/-/@acme/widget-1.0.0.tgz`) where the npm registry strips it (`@acme/widget/-/widget-1.0.0.tgz`). Declaring it lets pnpm rebuild that URL, so it is omitted from `pnpm-lock.yaml` instead of being written out for every scoped package [pnpm/get-npm-tarball-url#16](https://github.com/pnpm/get-npm-tarball-url/issues/16).
  - **`scopes`** lists the `@`-prefixed scopes that resolve from this registry. A bare `'@'` is the scope-less default registry, the one the `registry` setting names.
  - **`prefix`** is the alias a dependency addresses this registry by, as in `"foo": "work:^1.0.0"`.

  The layout is never inferred from the registry URL, so nothing changes unless you declare it; `registry.npmjs.org` continues to behave as `npm` without being declared. Because the lockfile depends on `serverType`, it is read from `pnpm-workspace.yaml` only — a `serverType` in the global `config.yaml` is ignored, so one developer's machine cannot shape a lockfile their collaborators read back with a different layout. Credentials are rejected in this setting, in a key as well as in a field, and still belong in `.npmrc`. An entry that routes nothing to itself and matches no configured registry is reported as a warning rather than silently ignored.

  ### Migrating

  The older `registries` shape, a map of `<scope>: <url>` strings, still works and needs no change:

  ```yaml
  registries:
    '@acme': https://npm.acme.example/
  ```

  `namedRegistries` is deprecated in favor of the `prefix` field, and is still read for prefixes `registries` does not declare.

  `toLockfileResolution` and `isCanonicalRegistryTarballUrl` now take their registry and layout as an options object rather than positional arguments, so `@pnpm/lockfile.utils` and `@pnpm/resolving.tarball-url` get a major bump.

- `pnpm root -g` and `pnpm bin -g` now print warnings to stderr instead of stdout, so their stdout stays a clean, machine-readable path. Previously, running either command with `--global` in a project that pins a package manager (e.g. via the `packageManager` field) printed a warning like `[WARN] Using --global skips the package manager check for this project` ahead of the path, breaking programs that capture the output as a path [#13672](https://github.com/pnpm/pnpm/issues/13672).

  In pnpm 12, `pnpm root -g` and `pnpm prefix -g` are now supported (they previously failed with `ERR_PNPM_CLI_ROOT_GLOBAL_UNSUPPORTED` / `ERR_PNPM_CLI_PREFIX_GLOBAL_UNSUPPORTED`), and the reporter output of `dlx`, `create`, `config`, `sbom`, `with`, `store`, `prefix`, `root`, and `bin` goes to stderr, matching pnpm 11.

- Added support for executing multiple scripts matching a RegExp passed to `pnpm run` (e.g., `pnpm run "/^build:.*/"`), running matched scripts in deterministic lexicographical order. Restored the `--sequential` (`-s`) CLI option for `pnpm run`, which forces `workspaceConcurrency` to 1 so that matched scripts run sequentially one by one across and within packages.

- Resolving a Node.js runtime version (`devEngines.runtime` / `runtime:` specifiers) is now much faster: the per-version release metadata is cached in the pnpm cache directory after its signature is verified, and an exact stable version such as `runtime:22.23.2` no longer downloads the Node.js release index. A pinned runtime whose metadata was fetched once resolves without any network access, which removes the noticeable delay on the first `node` invocation in a project pinning an already-downloaded runtime [#13899](https://github.com/pnpm/pnpm/issues/13899).

- Added the commands the Rust CLI was still missing:

  - `pnpm get <key>` and `pnpm set <key> <value>` — the top-level spellings of `pnpm config get` and `pnpm config set`.
  - `pnpm store status` — reports the packages whose files no longer match the store they were expanded from, failing with `ERR_PNPM_MODIFIED_DEPENDENCY`; and `pnpm store add <pkg>...` — fetches packages into the store without writing a manifest, a lockfile, or `node_modules`.
  - `pnpm env use --global <version>` and `pnpm env list [<selector>]`, the deprecated Node.js-only front end to `pnpm runtime`.
  - `pnpm edit`, `pnpm profile`, `pnpm token`, and `pnpm xmas` now fail with `ERR_PNPM_NOT_IMPLEMENTED` pointing at the npm CLI, instead of being taken for a package script.

- An install that resolves the dependency graph now reports the unmet peer dependencies it leaves behind, matching the TypeScript CLI. By default it warns once — `Issues with peer dependencies found. Run "pnpm peers check" to list them.` — and with `strictPeerDependencies` it fails with `ERR_PNPM_PEER_DEP_ISSUES` after the artifacts are written, listing every unmet peer. This covers `pnpm install`, `add`, `remove`, `update` and `--lockfile-only`; `pnpm dedupe` reported the same verdict already, and now shares the reporting with them. `peerDependencyRules` are applied before the verdict, so a rule that covers every issue leaves nothing to report, and a `--filter`ed install reports only on the projects it installed. An install that skips resolution — a frozen install, or one whose `pnpm-lock.yaml` is already up to date — reports nothing, as in the TypeScript CLI; `pnpm peers check` inspects such a tree [#14098](https://github.com/pnpm/pnpm/issues/14098).

- The `save-prefix` setting now accepts `=`: newly added dependencies are saved with an explicit `=` operator (`=1.2.3`) instead of the setting being silently treated as the default `^`.

- `pnpm self-update` no longer takes any instruction from the project it is run in:

  - pnpm is fetched through the same trusted registry and auth configuration used when switching pnpm versions, so a project `.npmrc` or `pnpm-workspace.yaml` can no longer redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is no longer loaded. Pnpmfiles from trusted sources (the `pnpmfile` setting, the global pnpmfile, config dependencies) still apply.
  - The `minimumReleaseAge` settings in `pnpm-workspace.yaml` no longer affect `self-update`. They still govern the project's own dependencies; for `self-update` the cooldown now comes from the built-in default, your global config, a `PNPM_CONFIG_*` environment variable, or a command-line flag. This fixes `self-update` failing inside a workspace that raises the cutoff while succeeding everywhere else, and stops a repository from either waiving the cooldown or keeping you on an outdated pnpm by raising it.
  - The same applies to the `trustPolicy` settings and to `ci`: a project can no longer weaken the trust check that guards the pnpm download, nor re-enable the confirmation prompt that a CI run suppresses.

  When `self-update` refuses a version that is younger than the cutoff, an interactive run now offers to update anyway; non-interactive runs still fail. CI never prompts, even on a runner that attaches a TTY.

- Added `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps` to the Rust pnpm CLI and its N-API bindings. Slow registry metadata requests and tarball downloads now emit pnpm-compatible warnings without exposing URL credentials, query parameters, fragments, or control characters [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).

- `pnpm stage approve` now approves several staged packages at once. Run it without a stage id to pick from the staged versions interactively, or pass a list of stage ids. The whole batch is approved with a single one-time password, and pnpm asks for a new one only once the registry stops accepting it. Inside a workspace, the selected packages are approved in dependency order, and a package whose workspace dependency could not be approved is skipped instead of being published against a dependency that never reached the registry.

- Added PnP install materialization and fixed recovery from expired module caches and broken private lockfiles.

- Added filtered and split SBOM generation with per-project lockfiles, including reachable workspace projects and incomplete-graph validation [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- Added a `--changeset` flag to `pnpm update`. Set `update.changeset` to `true` in `pnpm-workspace.yaml` to enable this behavior by default, and use `--no-changeset` to override the setting for one update. After the update completes, pnpm writes a `.changeset/pnpm-update-<suffix>.md` file declaring a patch bump for every workspace package whose `dependencies` or `optionalDependencies` were changed by the update and a major bump when `peerDependencies` changed, including packages that consume an updated catalog entry via the `catalog:` protocol. Private packages, packages without a name, and packages listed in the `ignore` array of `.changeset/config.json` are skipped. If `.changeset/config.json` does not exist, a warning is printed and no changeset is generated.

- Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.

- `pnpm install` now fails with `ERR_PNPM_UNUSED_PATCH` when an entry in `patchedDependencies` doesn't match any installed package. Set `allowUnusedPatches: true` in `pnpm-workspace.yaml` to get a warning instead, matching pnpm 11 [#11633](https://github.com/pnpm/pnpm/issues/11633).

- Added `virtualStoreType`, which names where the virtual store lives — one store per machine, or one per project:

  ```yaml
  virtualStoreType: global   # or: project
  ```

  It is the canonical spelling of `enableGlobalVirtualStore`, which keeps working. When a project sets both, `virtualStoreType` wins. It can also be set through `PNPM_CONFIG_VIRTUAL_STORE_TYPE` and read back with `pnpm config get virtualStoreType`. The default is unchanged — `project`, so the shared store stays opt-in.

  The setting is independent of `nodeLinker`. `isolated` and `pnp` both work with either store type, and `hoisted` writes no virtual store at all, so it is unaffected.

### Patch Changes

- pnpm now decodes package archives using a bounded amount of memory, whatever an archive or a registry claims about its size. A gzipped tarball that inflates past what a whole-archive decode may hold is extracted as a stream instead, a response body that keeps arriving is extracted while it downloads rather than accumulated in full, and a zip entry is read only as far as the size its archive records. No archive is refused for being large — everything that installed before still installs.

- `pnpm config delete` no longer leaves a blank line at the end of `pnpm-workspace.yaml` when it removes the last setting in the file. Because that blank line stayed behind, a later `pnpm config set` separated its new setting from it and the file ended up with two blank lines before the added setting.

- `pnpm fetch` now links each virtual-store package's dependency bins, so a dependency's lifecycle script can invoke a sibling dependency's bin. Previously a `postinstall` calling one — as `unrs-resolver` does with `napi-postinstall` — failed with `command not found` in the Docker "fetcher stage" shape (a lockfile with no project manifest), while `pnpm install` against the same lockfile succeeded [#14174](https://github.com/pnpm/pnpm/issues/14174).

- Fixed filtered `install`, `add`, `update`, and `remove` commands in shared-lockfile workspaces to install the workspace root alongside the selected projects [pnpm/pnpm#13397](https://github.com/pnpm/pnpm/issues/13397).

- `bundledDependencies` is no longer dropped from `pnpm-lock.yaml` when an install rewrites it. Bumping a single dependency stripped the field from every unrelated entry that carried it, and a `libc` recorded as a plain string was rewritten as a list [#14153](https://github.com/pnpm/pnpm/issues/14153).

- Deprecated the pnpmfile `filterLog` hook in pnpm v12. The Rust CLI ignores it and emits a warning.

- `--production` is accepted again as an alias of `--prod` on `install`, `fetch`, `prune`, `update`, `list`, `why`, and `sbom`, and the install that `verifyDepsBeforeRun` reproduces is now spelled with `--prod`. `pnpm run` no longer aborts with "unexpected argument '--production' found" after a production-only install [#14147](https://github.com/pnpm/pnpm/issues/14147).

- Forward `patchedDependencies` hashes and `packageExtensions` to pnpr so server-side resolution preserves patches and package extensions in the lockfile and installed packages.

- pnpm now runs the pnpm version that `pnpm-lock.yaml` records for a `devEngines.packageManager` range, instead of any pnpm on `PATH` the range also allows. A project pinning `^12.0.0-rc.3` with `12.0.0-rc.11` recorded went on running an older `12.0.0-rc.7`.

  Version pins are also matched the way npm's `semver` matches them: a prerelease no longer counts as satisfying a range asking for something later — `12.0.0-rc.7` against `>=12.0.0-rc.9` or `^12.0.0` — and a bound that omits a component is read as npm reads it, so a `<=22` engine range accepts 22.5.0. This applies to the package manager check and to `engines` checks alike.

- Enforce `allowBuilds` when a prepared git dependency is reused from the shared store, and use the lockfile's canonical git resolution ID in approval suggestions.

- Topologically sorting workspace projects now runs in linear time, fixing installs and lockfile updates that stalled for seconds on workspaces with thousands of projects forming deep dependency chains [#14149](https://github.com/pnpm/pnpm/issues/14149), [#14151](https://github.com/pnpm/pnpm/issues/14151).

- `pnpm add yarn@<version>` now records just the resolved version in the `packageManager` field, without corepack's integrity hash. Corepack verifies the release it downloads on its own, so the hash only added a second copy of that information to a field pnpm never verifies.

- `pnpm add` no longer re-resolves the dependency graph when `pnpm-lock.yaml` already holds a version satisfying the request — promoting a transitive dependency to a direct one, or adding to a second workspace package what a first one already depends on, now only saves the dependency in `package.json` and records its importer entry. A satisfying locked version is necessary but not sufficient: the install still falls back to a full resolution for a dist tag, an alias, a `workspace:`/`catalog:`/git/tarball specifier, `--save-peer`, an overridden package, a `catalogMode` other than `manual`, and — under `resolutionMode: time-based` or `lowest-direct`, which resolve a direct dependency to the low end of its range — a range several locked versions satisfy.

- `pnpm add` no longer drops the other dependency groups from the install: adding a package with `optionalDependencies` no longer leaves dangling optional-dependency symlinks in the virtual store (`pnpm add -g @openai/codex` produced a `codex` bin that failed with "Missing optional dependency `@openai/codex-darwin-arm64`"), and a production `pnpm add` no longer removes the project's `devDependencies` from `pnpm-lock.yaml` and `node_modules`.

- `pnpm add` with `--save-dev`, `--save-optional`, or `--save-prod` now moves an already-declared dependency to the target group instead of leaving a duplicate entry in its old group, matching pnpm.

- `pnpm add <pkg>` without a `--save-*` flag now updates an already-declared dependency in the group it occupies (`devDependencies` / `optionalDependencies`), matching pnpm, instead of always saving it into `dependencies`.

- An aliased dependency of a protocol that resolves under its own package name — `jsr:` and the named registries — is recorded in the lockfile importer again. `"bar-from-jsr": "jsr:@pnpm-e2e/bar@1.0.0"` resolved and installed, but the importer stayed empty, so nothing reading direct dependencies out of the lockfile (`outdated`, `update`, `licenses`, dedupe, frozen-install verification) could see it [#13362](https://github.com/pnpm/pnpm/issues/13362).

- An `allowBuilds` entry with the `set this to true or false` placeholder pnpm scaffolds no longer makes every command in that workspace fail with a config-parse error [#13322](https://github.com/pnpm/pnpm/issues/13322). An undecided entry now leaves the package under the default-deny build policy, as pnpm does.

- `allowBuilds` entries can now approve git-hosted packages that pnpm downloads as a tarball, such as `github:` dependencies (which are fetched from `codeload.github.com` rather than cloned), by their repository URL without the resolved commit hash. This matches the hashless `git+` matching already supported for cloned git dependencies. For example:

  ```yaml
  allowBuilds:
    "foo@git+https://github.com/org/foo.git": true
  ```

  This approves the package whether pnpm clones it or downloads a tarball, so the entry no longer has to be updated every time the pinned commit changes. GitLab and Bitbucket tarball downloads are matched the same way. Approving or denying a specific resolved commit by its full tarball dep path continues to work.

- Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it [#13073](https://github.com/pnpm/pnpm/issues/13073). Reading a lockfile through a symlink is allowed again, and an install that leaves the lockfile unchanged no longer rewrites it, so `--frozen-lockfile` no longer needs to write at all. Writing a *changed* lockfile through a symlink is still refused, as that would redirect the write onto the symlink's target.

- Installing a workspace whose projects auto-install peer dependencies is substantially faster. Each round of the peer-hoist loop no longer scans the whole workspace once per project, so the cost of resolution grows with the workspace instead of with its square.

- Kept pending build approvals available after removing an unrelated dependency.

- `pnpm approve-builds` now removes `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies` from `pnpm-workspace.yaml` when it writes `allowBuilds`. Those settings were replaced by `allowBuilds` in pnpm 11 and silently ignored since, so a workspace migrated from pnpm 10 kept them around looking active.

- `pnpm install` now detects a `supportedArchitectures` change and re-evaluates previously skipped platform-specific optional dependencies, instead of reporting the project as up to date and leaving the packages for the old architecture set in place.

- Two `pnpm install` resolution fixes that made large workspaces such as [Astro](https://github.com/withastro/astro) produce a different `pnpm-lock.yaml` than pnpm 11 [#13334](https://github.com/pnpm/pnpm/issues/13334):

  - A scoped workspace package referenced through the `file:` protocol (`"@test/pkg": "file:./pkg"`) is recorded as a `link:` again instead of being copied in as a `file:` snapshot.
  - `bundledDependencies` / `bundleDependencies` are no longer resolved as dependencies of their own. npm ships them inside the package's tarball, so installing them again added packages the lockfile should not contain (for example `napi-wasm` under `@parcel/watcher-wasm`).

- Global installs now switch over atomically. The command shims in the global bin directory point at a stable per-package link rather than at the directory a particular install produced, so `pnpm add -g` and `pnpm update -g` activate a new version by moving that one link instead of rewriting every shim. A command can no longer be missing from `PATH` while an install is in progress, and a failed install leaves the previous version in place.

- `pnpm audit --fix` and `pnpm audit --fix update` no longer add `minimumReleaseAgeExclude` entries for patched versions that were published before the `minimumReleaseAge` cutoff. The publish time of each minimum patched version is now checked against the registry metadata, and only versions young enough to be blocked by the age gate get an exclusion entry [#11563](https://github.com/pnpm/pnpm/issues/11563).

- `pnpm audit` no longer reports a patched version that was never published or is deprecated. The inferred patched range (e.g. `>=4.17.24` from `<=4.17.23`) is now checked against the registry packument, and the report is corrected to the lowest non-deprecated published version that satisfies it (e.g. `>=4.18.1` when `4.17.24` does not exist and `4.18.0` is deprecated). When no published version satisfies the range, the report shows `Patched versions: None`. This also prevents `pnpm audit --fix` from adding overrides or `minimumReleaseAgeExclude` entries for patches that do not exist [#13824](https://github.com/pnpm/pnpm/issues/13824).

  `pnpm audit --fix` and `pnpm audit --fix update` no longer add a `minimumReleaseAgeExclude` entry when the registry packument shows that the minimum patched version was never published. Previously such entries were written for versions that do not exist, which would have let a later publish of that version bypass the `minimumReleaseAge` gate [#11563](https://github.com/pnpm/pnpm/issues/11563).

  The `--json` output of `pnpm audit` now returns `patched_versions: null` for advisories whose inferred patch is not available (never published, skipped, yanked, or deprecated), making it easier for tooling to distinguish "no fix available" from "fix available at version X".

- Archive entries whose paths use `\` as a separator are now read the same way pnpm reads them. A nested path spelled `bin\tool.js` by Windows publishing tooling resolves to `bin/tool.js`, and a path traversal spelled with backslashes is rejected instead of being stored verbatim.

- Limit registry-provided gzip preallocation hints to 64 MiB so oversized `dist.unpackedSize` values cannot trigger excessive eager allocation.

- Bounded the number of requests in flight to the `.pnpmfile.cjs` worker process. An install that runs the `readPackage` hook for thousands of packages at once no longer risks failing with `ERR_PNPM_PNPMFILE_FAIL` on a hook timeout spent waiting in the queue rather than running the hook, and holds fewer copies of the manifests it is hooking while it waits.

- Installing a dependency chain whose packages carry peer dependencies no longer expands exponentially with the depth of the chain. A single project with a single such dependency could exhaust memory before finishing; it now resolves in tens of megabytes.

- Fixed `pnpm patch-commit` in project and edit paths containing non-ASCII characters.

- Executables that a package ships inside its own tarball (`bundledDependencies`) are linked again into that package's `node_modules/.bin`, under both the isolated and the hoisted node linker. A package that declares `bundleDependencies: true` instead of a list of names is now recorded in `pnpm-lock.yaml` the way pnpm 11 records it, and such a lockfile can be read back.

- Aligned `pnpm licenses list --json` package metadata and license-group ordering with the TypeScript CLI.

- Use macOS native DNS resolution with bounded concurrency so installs respect scoped and VPN-provided resolvers.

- Avoided optimistic repeat-install shortcuts when a lockfile contains merge conflict markers.

- Fixed `pnpm --filter <package> run` to list the selected package's scripts and root workspace scripts when no script name is specified.

- Fixed recovery from interrupted dependency builds in the global virtual store, and made `pnpm fetch` populate the virtual store without linking dependencies into projects.

- Strip Unicode formatting characters from registry- and manifest-derived terminal output.

- Prevented dependency verification before scripts from rewriting an up-to-date lockfile.

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` under `catalogMode: strict` no longer fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` when the catalog entry is a range that the wanted version satisfies. The dependency keeps using the catalog; only a version that really falls outside the catalog's range is rejected [#13715](https://github.com/pnpm/pnpm/issues/13715).

- Fixed resolving the `chcp` command on Windows during `pnpm setup` by looking for `chcp.com` before `chcp` [pnpm/pnpm#13991](https://github.com/pnpm/pnpm/issues/13991).

- Fixed `minimumReleaseAge` fallback for custom dist-tags so the selected version does not exceed the registry’s original tag target.

- Fixed `pnpm install` in CI to use frozen lockfile mode by default when an existing `pnpm-lock.yaml` is non-empty. An outdated lockfile now fails without being rewritten, while projects without a lockfile or with an empty lockfile can still generate one.

- `pnpm setup` now removes leftover v10-layout shims at the top of `PNPM_HOME`, so `pnpm self-update` no longer warns about a v10 installation layout after PATH has been migrated to the v11 `PNPM_HOME/bin` layout. Applies to both the TypeScript CLI and pacquet.

  In the TypeScript CLI, `self-update` also no longer treats a dangling legacy shim (one whose install target was garbage-collected) as a real v10 layout, so the warning can no longer fire on dead shim files.

  Closes pnpm/pnpm#12496.

- Re-fetch full registry metadata when `minimumReleaseAge` is enabled and an abbreviated packument's `time` map omits timestamps for some versions. This prevents mature versions from being filtered out and resolution from falling back to the lowest matching version [pnpm/pnpm#13741](https://github.com/pnpm/pnpm/issues/13741).

- A changed `catalogs` or `pnpm.overrides` block no longer has to be the only change for `pnpm install` to update the lockfile in place. Editing an override while also removing a dependency, or changing a catalog entry in the same commit as a range bump, is now absorbed in one pass instead of re-resolving the whole dependency graph [#13799](https://github.com/pnpm/pnpm/issues/13799).

  Fixed the lockfile an in-place override update wrote when the overridden package was also a catalog entry: the entry kept the version it had before the override moved the package. The same could happen in reverse, when a catalog entry moved a package an override pins. Both cases now re-resolve instead.

- `pnpm install` now updates the lockfile in place even when several kinds of changes happened since the last install — for example a removed dependency together with a widened `ignoredOptionalDependencies` list, or a dependency edit alongside a patch or settings change. Previously any combination of changes forced a full re-resolution [#13763](https://github.com/pnpm/pnpm/issues/13763).

- Concurrent commands in a repository that pins `packageManager` no longer race while installing the pinned pnpm version on a cold cache [#13322](https://github.com/pnpm/pnpm/issues/13322). A task runner spawning several `pnpm run` children at once could previously fail with "failed to remove existing directory … prior to swap", or leave a child looking for a binary another process had just unlinked.

- A config dependency carrying an inline integrity (the `<version>+<integrity>` form, or the object form without a `tarball`) now takes its tarball URL from the registry's packument instead of deriving it from the registry URL, so migrating one costs an extra metadata request. On a registry that serves tarballs from a path pnpm cannot derive, GitLab's group endpoint for one, installing such a config dependency failed with a 404 while the same package installed fine as a regular dependency [#13765](https://github.com/pnpm/pnpm/issues/13765).

- Config-load warnings, such as the warning about install settings left under the `pnpm` field of `package.json`, are printed to stderr instead of stdout [#13361](https://github.com/pnpm/pnpm/issues/13361).

- Fixed pnpm v11 incorrectly reporting `confirmModulesPurge` as unrecognized when set in `pnpm-workspace.yaml`. The Rust CLI now identifies the unsupported option as a pnpm v11 setting instead of suggesting an unrelated setting.

- A `+<algorithm>.<hash>` build in a `devEngines.packageManager` version no longer makes `pnpm install --frozen-lockfile` fail with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` on a lockfile a plain install kept rewriting identically [#14124](https://github.com/pnpm/pnpm/issues/14124).

- Corepack can run pnpm 12 again [#13018](https://github.com/pnpm/pnpm/issues/13018). Corepack installs no dependencies and runs no lifecycle scripts, so the native binary that the `pnpm` package normally receives from its platform-specific optional dependency was never there, and `corepack use pnpm@next-12` failed with `MODULE_NOT_FOUND`. The package now ships the `bin/pnpm.mjs` and `bin/pnpx.mjs` entry points Corepack looks for; they fetch the pinned native binary on first use — verified against npm's signature and checksum, honouring `COREPACK_NPM_REGISTRY` and the rest of Corepack's registry environment — and hand over to it. Installing pnpm with a package manager is unaffected and still runs the binary directly, with no Node.js startup in between.

- A custom fetcher can no longer replace the archive integrity that `pnpm-lock.yaml` pins: the locked value is restored after a `canFetch` or `fetch` hook rewrites the resolution, and delegating a locked archive to a directory or git source now fails instead of installing unverified content.

  The Rust CLI now also loads the pnpmfiles named by the `pnpmfile` setting (a single path or an ordered list), and hands custom fetchers native `localTarball` and `remoteTarball` callbacks — including on a fresh install that has to compute a missing tarball integrity, which is then reused by later offline installs. File maps a fetcher returns are accepted only when they match what those native callbacks extracted.

- Resolving peer dependencies in a workspace whose dependency graph contains many peer-dependency cycles now needs less than half the memory and finishes about twice as fast. Verdicts computed inside dependency cycles are now cached and reused for the occurrences they are provably valid for, instead of being recomputed for every occurrence.

- `pnpm dedupe --check` now reports what deduplication would change: the importer and package snapshot diff, the `ERR_PNPM_DEDUPE_CHECK_ISSUES` error code, and the warning that points at `pnpm peers check` when the install leaves peer-dependency issues behind. `pnpm peers check` is also accepted again — the subcommand spelling used on pnpm.io and in pnpm's own dedupe output — instead of failing with "unexpected argument 'check' found" [#13321](https://github.com/pnpm/pnpm/issues/13321).

- Fixed an injected workspace dependency (`injectWorkspacePackages: true`) incorrectly staying as `file:` instead of deduping back to `link:` when an unrelated, ordinary shared dependency resolved to a peer-suffixed variant for the target project's own copy but not for the injected occurrence. See pnpm/pnpm#10433.

- `pnpm dedupe` accepts the `pnpm install` options that pnpm documents for it — `--lockfile-only`, `--ignore-scripts`, `--offline`, and `--prefer-offline` — instead of rejecting them with `unexpected argument`. Without `--lockfile-only`, `pnpm dedupe` now also updates `node_modules`, as an install does [#14107](https://github.com/pnpm/pnpm/issues/14107).

- `pnpm install` and `pnpm dedupe` no longer eat all the available memory while resolving a graph in which many packages declare the same missing peer dependency, such as the `react` peer the `@radix-ui` packages share [#13786](https://github.com/pnpm/pnpm/issues/13786).

- Fixed a severe slowdown resolving large workspaces against registries whose abbreviated metadata lacks per-version `time` fields (such as `node-registry.bit.cloud`) while `minimumReleaseAge` is active. The resolver upgraded the abbreviated packument to full metadata once per *dependency edge* instead of once per package — re-requesting the same packument from the registry hundreds of times in a single install — and a `304 Not Modified` answer was never remembered, so the round trip repeated forever. The upgrade outcome is now cached for the rest of the install. On a 345-project workspace this cut a full resolution from 105 s to 36 s.

  Also stopped the resolver from deep-copying every workspace project manifest on each internal resolve-options clone (the workspace-packages map is now shared by reference).

- With `dedupeDirectDeps`, a project's symlink that becomes redundant — because the workspace root started providing the same dependency at the same resolution — is removed on the next install instead of surviving forever [#13775](https://github.com/pnpm/pnpm/issues/13775). The layout no longer depends on install history: an incremental install now ends up with the same `node_modules` a clean install of the same manifests produces.

- `pnpm dedupe` in the Rust engine now fails with `ERR_PNPM_PEER_DEP_ISSUES` when `strictPeerDependencies` is set and unresolved peer dependency issues remain after deduplication, matching the TypeScript CLI [#14099](https://github.com/pnpm/pnpm/issues/14099). Previously it only ever printed a warning, regardless of the setting.

- `pnpm deploy` now supports workspaces that use catalogs.

- `pnpm deploy --prod` and `pnpm deploy --no-optional` no longer list the excluded dependency groups in the deployed `package.json` and `pnpm-lock.yaml`. The deployed lockfile referenced packages that the deploy left out of its graph, so installing in the deploy directory afterwards created dangling symlinks [#13623](https://github.com/pnpm/pnpm/issues/13623).

- `pnpm deploy` injects workspace dependencies again, so the deploy directory is self-contained instead of symlinking back into the source workspace [#13754](https://github.com/pnpm/pnpm/issues/13754). Enabling `injectWorkspacePackages` with `dedupeInjectedDeps` disabled now also rewrites already-linked workspace dependencies to injected copies.

- Fixed `pnpm deploy` with a shared lockfile so local `file:` tarball dependencies keep their package name in the generated deploy lockfile. This prevents warm-store deploys from failing with `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE` when the tarball filename includes the version.

- `pnpm deploy --no-optional` no longer writes a lockfile whose snapshots reference optional dependencies that the deploy excluded.

- `pnpm --filter . deploy` deploys the project in the current directory instead of the projects nested under it, so deploying the workspace root now copies the root project and installs its workspace dependencies [#13758](https://github.com/pnpm/pnpm/issues/13758). `pnpm deploy --legacy` no longer rewrites the source workspace's `pnpm-lock.yaml`.

- A deprecated package is reported once rather than once per workspace project that depends on it, and is no longer double-counted in the "deprecated subdependencies found" summary when it is also a direct dependency [#13322](https://github.com/pnpm/pnpm/issues/13322). Ignored build scripts are also listed with their `(patch_hash=…)` suffix, so two copies of a package that differ only by an applied patch are distinguishable.

- Fixed `pnpm install` writing a different `pnpm-lock.yaml` for an unchanged project depending on the order its dependencies happened to resolve in, which showed up as spurious lockfile diffs between installs.

- `pnpm install --frozen-lockfile` no longer re-imports a varying subset of packages on every repeat install of an unchanged project [#13316](https://github.com/pnpm/pnpm/issues/13316). The global-virtual-store directory of a package that takes part in a dependency cycle was derived from an order that changed from run to run, so those packages landed on a fresh slot each time; it is now derived deterministically and matches the directory pnpm itself computes.

- Fixed non-deterministic resolution on multi-project workspaces: two consecutive installs of the same inputs could bind peer-suffixed packages to different (still valid) providers, rewriting `pnpm-lock.yaml` on every install [#13567](https://github.com/pnpm/pnpm/issues/13567).

- Installing a workspace now produces the same `pnpm-lock.yaml` every time. Two installs of the same workspace could previously bind a peer dependency to a different — still valid — version, which changed the lockfile without anything in the project changing.

- `pnpm install --dev` and `pnpm deploy --dev` no longer install optional dependencies, and `--prod` now takes precedence when combined with `--dev`, matching the TypeScript pnpm CLI.

- Fixed `file:` dependencies not being re-copied when their source directory changed. A `file:` dependency is copied into the store at install time rather than symlinked, so editing the local package's files and running `pnpm install` again left the previous copy in place — the lockfile is unchanged by such an edit, so the install treated the tree as up to date.

- `pnpm dlx` and `pnpm create` no longer fail with "Failed to read patch file" in a project that has `patchedDependencies`. As in pnpm, the package dlx runs is installed unpatched.

- Removing a dependency from `package.json` and reinstalling no longer re-resolves the dependency graph. The importer's entry is dropped from `pnpm-lock.yaml`, anything it made unreachable is pruned, and a catalog entry that loses its last referent is removed — all without registry access. Installs still fall back to a full resolution when a package that stays resolves a peer dependency through the removed one, since that would change the surviving package's entry rather than only prune.

- The built-in compatibility database no longer adds dependencies that were detected by static analysis of published packages. Those entries named packages that are only imported for their types, so installing them was at best unnecessary and at worst broke the dependent: `@typescript-eslint/types` gained a `typescript` dependency resolved to the newest release, which put TypeScript 7 under older `@typescript-eslint` versions and made ESLint fail with "Cannot read properties of undefined (reading 'Intrinsic')". The database keeps its `@yarnpkg/extensions` entries and pnpm's own curated ones.

- Don't treat files like `license16.json` as a package license when deciding if the workspace LICENSE file should be included in the packed package.

- A dependency published with `"bin": ""`, such as `url-loader@1.1.2`, no longer fails the install with `ERR_PNPM_CMD_SHIM_PROBE_SHIM_SOURCE` [#13962](https://github.com/pnpm/pnpm/issues/13962). An empty `bin` declares no command, as it does in pnpm v11, so no shim is written for the package; a `directories.bin` entry on the same package is still linked.

- An empty `http-proxy`, `https-proxy`, `proxy`, or `no-proxy` value — from the `.npmrc`, `pnpm-workspace.yaml`, the CLI, or the `HTTP_PROXY` / `HTTPS_PROXY` / `PROXY` / `NO_PROXY` environment variables — no longer fails the install with `ERR_PNPM_INVALID_PROXY`. Empty settings read as unset, so a shell exporting `HTTP_PROXY=` disables the proxy, and an empty `proxy=` in the `.npmrc` no longer suppresses `HTTPS_PROXY` [#13533](https://github.com/pnpm/pnpm/issues/13533).

  `proxy=false` in the `.npmrc` or `proxy: false` in `pnpm-workspace.yaml` now turns proxying off instead of being read as a proxy host named `false`. `false` and `null` on `https-proxy` / `http-proxy` / `no-proxy` read as unset, and on the command line they are ordinary host names, since a flag carries its value verbatim.

- Dependencies declared with an empty version range (`"adler-32": ""`) install again instead of failing with `ERR_PNPM_NO_MATCHING_VERSION` [#13673](https://github.com/pnpm/pnpm/issues/13673). An omitted range means "any version", as it does in npm and pnpm v11, so packages that publish one — such as `js-xlsx`, `codepage`, and `ssf` — no longer need an `overrides` entry to install.

- When the pinned `packageManager` engine install cannot take its lock because the store cannot be written to, pnpm now reports that instead of quietly installing without the lock. A lock another process holds is unchanged — it is still waited for.

- The env lockfile no longer pins `@pnpm/exe` alongside `pnpm` when the wanted pnpm version is 12 or newer. From v12 the unscoped `pnpm` package is itself the native executable, so `@pnpm/exe` is not published for it and resolving it would fail. The engine identity check now verifies the native binary through whichever package ships it.

- Changing a catalog entry to a different exact version no longer re-resolves the dependency graph. The package is replaced in `pnpm-lock.yaml` directly, reusing the same check the `pnpm.overrides` fast path applies: every locked dependency of the package must still satisfy the new version's manifest. Installs fall back to a full resolution when anything other than the catalog reaches the package — an importer that depends on it directly, or another package that depends on it — since the graph would then need both versions.

- A dependency pinned to an exact version carrying semver build metadata (`"@parcel/codeframe": "2.0.0-canary.1718+d8408010f"`) installs again instead of failing with `ERR_PNPM_NO_MATCHING_VERSION` [#14096](https://github.com/pnpm/pnpm/issues/14096). npm strips build metadata when it publishes a version, so pnpm strips it from the version it looks up, matching npm and pnpm v11.

- With `excludeLinksFromLockfile` enabled, a `link:` dependency pointing inside the workspace is no longer treated as an external link when it resolves a peer dependency, so the peer suffixes it produces stay identical to an install with the setting off. Injected (`file:`) workspace dependencies are no longer affected by the setting either [#13556](https://github.com/pnpm/pnpm/issues/13556).

- Aligned deprecated package warnings with pnpm by reporting each package only on its first resolution and shortening direct dependency warnings during recursive installs.

- Speed up installs after compatible catalog or direct dependency range changes by retaining the locked version without resolving the dependency graph again.

- Speed up installs after safe override changes by reusing unambiguous compatible dependency resolutions, pruning obsolete dependencies, applying independent replacements and removals together, and handling parent-scoped `"-"` overrides without full lockfile resolution.

- Removing the last dependency that references a catalog entry via the fast lockfile update no longer leaves the stale catalog entry in `pnpm-lock.yaml`.

- Reduced the warm startup overhead of project-aware managed runtime shims.

- Reduced warm update overhead by limiting virtual-store bin linking and ignored-script build bookkeeping to packages materialized by the current install.

- Resolution on large peer-heavy workspaces got faster: a Bit workspace with 114 projects and ~21,000 lockfile entries resolves in ~13.4s instead of ~16.0s. The resolved dependency graph is unchanged.

- A package's `files` entries now match only at the package root, the way npm reads them. A bare `src` used to also match nested directories such as `example/src`, so a dependency installed from git could ship the repository's own example app. The same filter decides what `pnpm pack` and `pnpm publish` put in a tarball and what `pnpm deploy` copies, so those stop carrying the extra files too. Exclusions such as `!**/__tests__` and `!*.map` still match at any depth. A package already in the store keeps its old file set until it is fetched again.

- A `pnpm install --filter <selector>` run that has nothing to do now reports "Already up to date" without entering the install pipeline, the same way an unfiltered `pnpm install` already did [#14033](https://github.com/pnpm/pnpm/issues/14033).

- Fixed `404` errors when installing from a registry that serves scoped packages only from a percent-encoded path, such as GitHub Enterprise Server. Outside `registry.npmjs.org`, a tarball URL that encodes the scope separator as `%2f` or `%2F` is no longer mistaken for one that pnpm can rebuild from the package name, version, and registry, so it is kept in `pnpm-lock.yaml` and requested verbatim on the next install [#13534](https://github.com/pnpm/pnpm/issues/13534).

- `pnpm outdated --include-github-actions` no longer blocks on an interactive git credential prompt when a workflow uses a private action repo.

- Fixed warm side-effects cache reuse for git dependencies.

- `pnpm init` now pins the exact pnpm version instead of a `^` range, and records it in the `packageManager` field alongside `devEngines.packageManager`. Corepack reads only `packageManager` and accepts nothing but an exact version, so it rejected the generated `package.json` with "expected a semver version" [pnpm/pnpm#13969](https://github.com/pnpm/pnpm/issues/13969). A package created inside an existing workspace is still left unpinned — it follows the pin at the workspace root — and `--no-init-package-manager` still scaffolds a manifest without any pin. In pnpm 12, `pnpm init` also honors `initType` and its `--init-type` flag, so the manifest it writes is the same one pnpm 11 writes.

- Write blocked-build approval scaffolding to the discovered workspace manifest when using per-project lockfiles.

- On Windows, upgrading pnpm no longer leaves a stale `pnpm.ps1` behind. PowerShell resolves `pnpm.ps1` ahead of `pnpm.cmd`, so a shim written by an older installation kept running the previous version. Linking the pnpm CLI's bins now deletes it [#13919](https://github.com/pnpm/pnpm/issues/13919).

- Fixed an inconsistency where `minimumReleaseAgeExclude` (and `trustPolicyExclude`) wildcard/bare-name rules behaved differently in the evaluator and normalizer. A bare rule now consistently evaluates as matching every version, preventing unexpected behavior and silent widening of version policy exemptions when pnpm rewrites the workspace manifest [pnpm/pnpm#13725](https://github.com/pnpm/pnpm/issues/13725).

- Settings written to a `pnpm-workspace.yaml` block that uses inline (flow) YAML — `catalog: { foo: ^1.0.0 }`, `overrides: { foo: 1.0.0 }`, `minimumReleaseAgeExclude: [foo@1.0.0]` — are now edited in place instead of failing or corrupting the file. `pnpm audit`, `pnpm link`, `pnpm approve-builds`, `pnpm patch`, `pnpm add --config`, and catalog updates all keep the block's flow style, its other entries, and its comments [#14108](https://github.com/pnpm/pnpm/issues/14108).

- Bump version.

- Fixed frozen installs incorrectly treating equivalent Git dependency specifiers as a stale lockfile. See [#13039](https://github.com/pnpm/pnpm/issues/13039).

- Aligned `pnpm dedupe --check` progress and error output with the TypeScript CLI.

- Fixed nondeterministic peer bindings in large multi-project workspaces.

- A frozen install now fails when `autoInstallPeers`, `dedupePeers`, or `excludeLinksFromLockfile` has changed since `pnpm-lock.yaml` was written, instead of installing against a lockfile that no longer matches the settings. The error names the drifted setting, as `pnpm install --frozen-lockfile` has always done.

- `--frozen-lockfile` no longer rejects a lockfile pnpm just generated when `packageExtensions` adds a peer dependency to a workspace project. The peer is auto-installed and recorded in the importer entry, but the freshness check compared against the `package.json` on disk, which has no such peer, and reported the entry as a removed dependency [#13836](https://github.com/pnpm/pnpm/issues/13836).

- A frozen install no longer rewrites the `packageManagerDependencies` block of `pnpm-lock.yaml`. When the pnpm version pinned by `devEngines.packageManager` (or by `packageManager`) is missing from the lockfile or no longer matches it, `--frozen-lockfile` now fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` instead of resolving the version and saving it, so a manifest whose pin was bumped without regenerating the lockfile can no longer pass CI [#14009](https://github.com/pnpm/pnpm/issues/14009).

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when the pinned pnpm version recorded in `pnpm-lock.yaml` has to be re-resolved before it can be installed. It runs the pnpm version the lockfile pins and leaves the lockfile unchanged [#14124](https://github.com/pnpm/pnpm/issues/14124).

- A repeat `pnpm install --frozen-lockfile` is a no-op again when the project has a platform-incompatible optional dependency. The skipped package is kept in `node_modules/.pnpm/lock.yaml` (`.modules.yaml` is what records the skip), so the install can once more recognize an unchanged tree instead of re-running every lifecycle and dependency build script [#13312](https://github.com/pnpm/pnpm/issues/13312).

- Reject frozen installs when the current pnpmfile does not match the lockfile's `pnpmfileChecksum`.

- Fixed `pnpm licenses list` to report dependencies from every workspace project, exclude unsupported platform packages, and mark development dependencies.

- Setting both `autoInstallPeers: false` and `dedupePeerDependents: false` now leaves missing peers alone, instead of still installing the ones a version elsewhere in the workspace could satisfy.

- When a git-hosted dependency is blocked from running build scripts, the error now suggests an `allowBuilds` entry that actually approves it. It quoted the bare package name, which never matches a git-hosted package, so following the suggestion left the install failing the same way [#14002](https://github.com/pnpm/pnpm/issues/14002).

- A git-hosted dependency with no host archive (an ssh, self-hosted, or `git+file:` repo) whose package name matches the dependency's alias now records the bare `git+<repo>#<commit>` reference in the lockfile's importer entry, matching pnpm's `pnpm-lock.yaml` output instead of prefixing it with `<name>@`.

- A git dependency whose clone (or shallow fetch) fails now reports which package it belongs to, under the `ERR_PNPM_GIT_FETCH_FAILED` code, with credentials in the repository URL redacted. When the lockfile records an SSH remote, the error also explains that fetching it needs an SSH key for that host, and that a lockfile entry written before pnpm v11.21 can be re-recorded over HTTPS with `pnpm update <package>` [#13743](https://github.com/pnpm/pnpm/issues/13743).

- Fixed `pnpm install` aborting with a panic when a project depends on a git-hosted package [#13040](https://github.com/pnpm/pnpm/issues/13040).

- A private git-hosted dependency resolved over HTTPS with an embedded auth token (`git+https://<token>@github.com/owner/repo.git`) is now recorded as a `type: git` resolution against the authenticated remote, instead of being rewritten to the host's public archive URL (a `codeload.github.com` tarball) that carries none of those credentials and so could not be fetched.

- An `integrity` recorded on a git dependency's resolution (`resolution: {type: git, repo, commit, integrity: sha512-…}`) is no longer treated as a checksum. pnpm never verifies a git checkout against such a hash — the commit pins the content — so it is now dropped when the lockfile is rewritten, and `pnpm sbom` no longer republishes it as a CycloneDX/SPDX checksum. Lockfiles carrying one also load again instead of failing with `ERR_PNPM_BROKEN_LOCKFILE` [#13042](https://github.com/pnpm/pnpm/issues/13042).

  `pnpm sbom` now also publishes the checksum of a `type: binary` runtime archive, which pnpm does verify.

- A git dependency whose `git ls-remote` fails now reports the `ERR_PNPM_GIT_RESOLVE_FAILED` code, naming the dependency instead of printing a bare `git` invocation, with credentials in the repository URL redacted. A specifier that does not ask for SSH resolves over HTTPS, because the URL recorded in the lockfile has to work on every machine that installs it, so the error explains how to substitute the transport on a machine that can only reach the host over SSH (`git config --global url."git@<host>:".insteadOf "https://<host>/"`) [#13743](https://github.com/pnpm/pnpm/issues/13743).

  A missing `git` executable is reported as one, instead of surfacing the raw failure to start the process.

  Credentials embedded in a git specifier are redacted from the "Could not resolve \<ref\> to a commit of \<repo\>" errors too.

  Resolving a public repository makes one `git ls-remote` round-trip instead of two.

- A git dependency installed over HTTPS from a hosted repository now keeps its branch, tag, or version range in the specifier recorded in `package.json`. It was written back without one, so the next `pnpm update` moved the dependency to the repository's default branch [#13999](https://github.com/pnpm/pnpm/issues/13999).

- Added support for the `globalPnpmfile` setting, which names a user-level pnpmfile that runs for every project ahead of the project's own. Like pnpm, it is left out of the lockfile's `pnpmfileChecksum`, so editing it does not decide whether a lockfile is still current. `pnpmfile` and `globalPnpmfile` are now also readable from `PNPM_CONFIG_PNPMFILE` and `PNPM_CONFIG_GLOBAL_PNPMFILE`.

- Fixed `pnpm update --global --latest` failing with a 404 error when a globally installed package was not added from the registry by name. Packages installed from a local path (`link:`/`file:`), a git repository, a tarball URL, an `npm:` alias, or a named registry now keep their spec during a global update instead of being looked up by name in the default registry. See pnpm/pnpm#12854.

- Fix recursive `pnpm update <name>@<version>` so an exact pinned update stays scoped to the requested version line: copies of the same package on another major line — or, for a `0.x` request, another minor line — keep their locked resolution instead of being re-resolved along with the target.

- `pnpm install` after moving a dependency between `dependencies`, `devDependencies`, and `optionalDependencies` now updates the lockfile in place instead of re-resolving the whole dependency graph [#13696](https://github.com/pnpm/pnpm/issues/13696).

- When a dependency's build script fails under `enableGlobalVirtualStore`, the global virtual store directory it was being built in is now removed for scoped packages too. Previously the cleanup resolved one directory level short of the hash directory for a scoped name, leaving a half-built directory behind that later installs would reuse.

- `ng build` and `nuxt build` now work under the global virtual store: pnpm's built-in compatibility extensions add the `tslib` dependency that `@angular/build` uses without declaring and the `unplugin` dependency that `@nuxt/vite-builder` v4 uses without declaring.

- Concurrent installs sharing a global virtual store no longer fail with `failed to remove existing directory ... prior to swap: Directory not empty`, and no longer briefly remove a package directory another process is reading.

- Fixed installs under `enableGlobalVirtualStore` failing with `failed to remove existing directory ... prior to swap: Directory not empty` (or `No such file or directory`) when peer variants of an injected `file:` dependency hash to the same slot. The link pass now materializes each unique slot directory once instead of racing one force-mode import per peer variant against the same path.

- Fixed `link:` dependencies under `enableGlobalVirtualStore` so linked children are materialized and slots remain isolated by their resolved link targets.

- Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

  Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.

- Fixed installs failing on Windows when the global virtual store is enabled. The `<store>/links/<scope>/<name>/<version>/<hash>` slot path is formatted with `/` separators (it doubles as a cross-platform canonical id), and those forward slashes were reaching `CreateSymbolicLinkW`, which rejects forward-slash paths with `ERROR_DIRECTORY` (os error 267). The slot path is now expanded into native path components before any filesystem call.

- The token poll for web-based authentication no longer reads the body of non-OK or still-pending (HTTP 202) responses, and caps the token response body it does read at 64 KiB, so a malicious or compromised registry cannot exhaust memory through the poll [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).

- A headless install (`--frozen-lockfile`) now creates the command shims for a publicly hoisted workspace package's `bin`, matching what a normal install already did and what pnpm's own headless install does. Previously those shims were missing until the next non-frozen install.

- The held-back-update warning printed by `pnpm update` no longer fires when `minimumReleaseAge` is the actual reason a newer version was not picked. The warning's baseline now applies the same maturity cutoff as the pick itself, so it no longer wrongly attributes the hold-back to "your manifests and already installed dependencies" or recommends an override that would defeat the age gate. See pnpm/pnpm#13071.

- A missing required peer is no longer auto-installed as a prerelease that its declared range rejects. A package peer-depending on `^29.0.0 || ^30.0.0` next to a `30.0.0-alpha.6` pulled in elsewhere in the graph now resolves a stable `29.x`/`30.x` from the registry instead of adopting the alpha [#13341](https://github.com/pnpm/pnpm/issues/13341).

- Resolving a workspace whose dependency chains are deep is faster: deciding which missing peer dependencies another project's resolution already covers now answers once per shared chain segment instead of once per report.

- A hoisted-linker install no longer fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` when an optional dependency's snapshot is absent because it was skipped on a previous install.

- Under `nodeLinker: hoisted`, a dependency declared against a peer-resolution variant of a package version is no longer dropped from the installed layout. All variants of a version share one hoisted copy, and edges pointing at any of them now resolve to it, so the depending project keeps the package in its `.package-map.json` and the depending package keeps it in its `node_modules/.bin`.

- Under `nodeLinker: hoisted`, peer-resolution variants of an injected directory dependency (a `file:` snapshot) are materialized as separate copies again instead of collapsing onto the first-seen variant. Each copy keeps its own peer-resolved dependency set, so a project pinning one peer version no longer resolves another project's variant — Bit root components with conflicting peers across injected copies rely on this.

- Fixed patched dependencies being applied to only one copy of a package under `nodeLinker: hoisted`. When a version conflict kept a patched package out of the root `node_modules`, the hoisted layout nested a copy of it under each consumer that needed it, but only the first copy was patched — every other copy silently ran the unpatched code the patch existed to replace. The same gap applied to a reinstall served from the side-effects cache. Every copy is now patched, matching `nodeLinker: isolated` and pnpm's behavior.

- Auto-installed peer dependencies are no longer resolved to their lowest satisfying versions under `resolutionMode: lowest-direct` or `time-based`. A hoisted peer is not a dependency the project declares, so it resolves like a transitive dependency — to the highest version satisfying the peer range (under `time-based`, the highest within the publish-date cutoff) [#13871](https://github.com/pnpm/pnpm/pull/13871).

- A repeat `pnpm install` with `nodeLinker: hoisted` is a no-op again when a workspace package declares the dependencies [#14001](https://github.com/pnpm/pnpm/issues/14001). The hoisted linker installs them into the root `node_modules`, but the up-to-date check previously looked under each package's own `node_modules` and reinstalled the whole tree every time. A hoisted install also no longer reports the packages it just wrote as broken.

- `node-linker=hoisted` installs no longer produce broken layouts on graphs with version conflicts. Three hoister fixes, aligning with `@yarnpkg/nm` (which the TypeScript CLI delegates to):

  - A version-conflicted package depended on by several packages kept its conflicting transitive dependencies under only one of the dependents, so requiring them through any other dependent resolved the wrong (root-hoisted) version — for example an ESM `parse-entities@4` resolving `character-entities-legacy` v1 instead of v3, which crashes with `ERR_IMPORT_ATTRIBUTE_MISSING` on Node.js 22. Hoist decisions are now made per parent path on decoupled copies (ports upstream's `decoupleGraphNode`).
  - Peer-resolution variants of one package version now collapse onto a single copy (ports pnpm v11's `depPathByPkgId` mapping) instead of conflict-nesting a copy under every dependent — on peer-variant-heavy graphs (such as `bit`'s) the old behavior also made the per-path walk explode.
  - Hoisting no longer shadows names a subtree resolves through an ancestor directory: a candidate is refused when a nearer ancestor holds a different version of its name (upstream's "filled by parent" scan) or when the hoist root's subtree already resolves that name from above (upstream's `usedDependencies` gate).

- `pnpm add` and `pnpm update` now honor the `saveExact` setting; previously only the `--save-exact` flag was respected.

- Fixed parsing very large lockfiles that exceed the YAML parser's default 64 MiB scalar-text budget.

- `pnpm install` now announces `Lockfile is up to date, resolution step is skipped` whenever the headless installer runs — including installs that materialize a cold `node_modules` from an up-to-date lockfile and `--filter` subset installs — matching the TypeScript CLI. `pnpm fetch` prints `Importing packages to virtual store` on that path instead.

- Conditional metadata requests send `If-Modified-Since` as an HTTP-date instead of the mirror's raw ISO-8601 `modified` value, so registries can answer `304 Not Modified` instead of re-serving the full packument [#13104](https://github.com/pnpm/pnpm/issues/13104).

- `pnpm install` no longer fails when `pnpm-lock.yaml` exists but cannot be parsed. Matching the TypeScript CLI, the install now prints an "Ignoring broken lockfile" warning, resolves dependencies from the manifests, and rewrites the lockfile. `--frozen-lockfile` still fails on a broken lockfile.

- `--ignore-pnpmfile` is accepted again, on every command pnpm takes it on: `install`, `add`, `update`, `dedupe`, `fetch`, `unlink`, `deploy`, `ci`, and `install-test` [#13808](https://github.com/pnpm/pnpm/issues/13808). The flag skips every pnpmfile hook the command would otherwise run: neither the workspace `.pnpmfile.cjs` nor the pnpmfiles of config dependencies are loaded, so no `readPackage`, `updateConfig`, `afterAllResolved`, custom resolver, or custom fetcher runs.

- `ignorePnpmfile` can now be set in `pnpm-workspace.yaml` and read from `PNPM_CONFIG_IGNORE_PNPMFILE`, not only passed as `--ignore-pnpmfile`, so a project or a machine can turn pnpmfile hooks off once instead of adding the flag to every command. The flag still applies on top. As in pnpm, the global `config.yaml` cannot set it: a pnpmfile belongs to the project that ships it.

- The resolved dependency graph and lockfile no longer depend on the order in which workspace projects are listed or discovered: importers are processed in project-id order, so reordering the `packages` globs in `pnpm-workspace.yaml` (or any other change to project listing order) produces a byte-identical lockfile [#13846](https://github.com/pnpm/pnpm/issues/13846). This also makes auto-installed peer placement, deprecation-warning attribution, and cycle back-edge bindings a function of the project set alone.

- Peer resolution on large workspaces got faster: each hoist round now refreshes its view of the dependency graph from what the round changed instead of re-reading every resolved package. The resolved dependency graph is unchanged.

- Changing `autoInstallPeers`, `dedupePeers`, `peersSuffixMaxLength`, `excludeLinksFromLockfile`, or `injectWorkspacePackages` no longer re-resolves the dependency graph when the lockfile proves the setting cannot affect it: no package or project declares a peer dependency for the peer settings, and no project depends on a directory or on another workspace project for the link and injection settings. The new setting is recorded in `pnpm-lock.yaml` and the install proceeds from the existing resolution. Every other case still falls back to a full resolution.

- Adding, editing, or removing an entry in `patchedDependencies` no longer re-resolves the dependency graph. Resolution never reads a patch — it only records the patch file's hash against the package it matches — so the install now rewrites the affected entries in `pnpm-lock.yaml` and materializes the patched package from the store instead. Installs still fall back to a full resolution when the patched package is reachable as a peer dependency, and when the new configuration would leave a patch unused while `allowUnusedPatches` is off, so `ERR_PNPM_UNUSED_PATCH` is still reported.

- `syncInjectedDepsAfterScripts` now removes the bin link of a bin the script dropped. Previously only new bins were linked, so a build step that stopped declaring one left its shim behind, pointing at a command that was no longer there.

- A lockfile entry for a git-hosted archive that records no `integrity` installs again instead of failing with `ERR_PNPM_MISSING_TARBALL_INTEGRITY`. Older pnpm versions wrote that shape for dependencies like `"ci-info": "watson/ci-info#f43f6a1c…"`, so any committed lockfile still carrying one could not be installed [#13308](https://github.com/pnpm/pnpm/issues/13308). The archive URL pins a full commit SHA, and pnpm fetches it without an integrity check.

  Every other remote tarball still has to carry an `integrity`, and the refusal now points at the repair: `pnpm clean --lockfile` followed by `pnpm install`.

  Error output no longer repeats the same message once per level of the internal error chain.

- `pnpm install` no longer crashes on a machine whose system certificate store is empty or absent — for example a minimal container or build sandbox that ships no CA certificates [#13588](https://github.com/pnpm/pnpm/issues/13588). Such a system now falls back to the Mozilla root certificates bundled into the binary, the same set Node.js ships, so both offline and online installs work again. Certificates from the system store, `NODE_EXTRA_CA_CERTS`, and the `.npmrc` `ca` / `cafile` settings keep taking precedence whenever any of them is available.

- The `Workspace` column of `pnpm update --interactive` now falls back to the project's path when its `name` is only whitespace, as it already did for a missing or empty one — all three render an equally blank label otherwise.

- Checking GitHub Actions dependencies for updates is now opt-in for every command. Neither `pnpm outdated` nor `pnpm update` reads the workflow files unless `--include-github-actions` is passed or `update.githubActions` is set to `true` in `pnpm-workspace.yaml`. Reading them runs `git ls-remote` against every referenced repository, which fails in environments where GitHub is not reachable the way pnpm assumes (a GitHub Enterprise Server, a custom certificate authority, or an offline network) [#13254](https://github.com/pnpm/pnpm/issues/13254).

  `pnpm outdated` accepts the `--include-github-actions` option too.

- `pnpm update --interactive` now groups the dependencies it offers by dependency type — `dependencies`, `devDependencies`, `optionalDependencies`, `peerDependencies`, and GitHub Actions each get their own heading — and lays each group out as a column-aligned table with a `Package`/`Current`/`Target`/`URL` header, instead of one flat list.

- `pnpm update --interactive` now measures its table in terminal columns rather than in characters. A package name, workspace name, or version containing wide characters (CJK, most emoji) no longer knocks its row's columns out of line with the rest of the group, and a wide character in a version no longer aborts the command with `Subject parameter value width cannot be greater than the container width` [#13357](https://github.com/pnpm/pnpm/issues/13357).

- `pnpm update --interactive` run inside a workspace now shows a `Workspace` column naming the project each outdated dependency was found in, so the same package outdated in several projects can be told apart.

- `pnpm update --no-save <pkg>@<version>` now keeps the manifest's declared importer specifier in `pnpm-lock.yaml` when the requested version satisfies that range, so a subsequent `--frozen-lockfile` install no longer fails because the lockfile records the requested version as the specifier.

- Fixed the order in which pnpm matches a lockfile's recorded tarball URL against known registry URLs. Two registry URLs of equal length were previously ordered arbitrarily, so which one a tarball URL matched could differ between runs.

- Fixed parsing large lockfiles that exceed the YAML parser's default structural budget [pnpm/pnpm#12857](https://github.com/pnpm/pnpm/issues/12857).

- `pnpm version -r` no longer writes a versioning-ledger entry with no consumed intents as a bare `intents:` key, which the next run failed to read with `ERR_PNPM_INVALID_VERSIONING_LEDGER`. Empty intent lists are now written as `intents: []`, and the ledger reader accepts the bare form left by earlier releases.

- Fixed dependency resolution letting the order in which concurrent resolutions finished decide the outcome. When one package was reached from several places, whichever occurrence got there first decided the versions its dependencies were recorded at, so repeated installs of the same project could produce different `pnpm-lock.yaml` files.

- Write single-value `libc` package metadata in the same scalar form as pnpm.

- Fixed `pnpm install` silently skipping a local `file:*.tgz` dependency: the package is now extracted into the virtual store, recorded under `packages:` and `snapshots:`, and linked into `node_modules` [#13379](https://github.com/pnpm/pnpm/issues/13379).

- Widening a dependency's range no longer leaves the project on an older version. The lockfile update now points the project at the highest version of that dependency already in the lockfile that satisfies the new range — matching what a full resolution records — instead of keeping the locked version whenever it happened to satisfy, which could leave a duplicate behind. A range change that only an already-locked version satisfies is now also handled without re-resolving [#13778](https://github.com/pnpm/pnpm/issues/13778).

- A frozen install whose recorded settings no longer match the configuration — `overrides`, `catalogs`, `patchedDependencies`, and the rest — now fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` naming the one setting that changed, instead of `ERR_PNPM_OUTDATED_LOCKFILE` with the whole map dumped [#13322](https://github.com/pnpm/pnpm/issues/13322).

- A registry dependency is now always recorded in `pnpm-lock.yaml` with an integrity hash, including under `--lockfile-only`. Packages from a registry that publishes no subresource-integrity metadata — `https://node-registry.bit.cloud/`, for one — were recorded without one, so the next `pnpm install --frozen-lockfile` failed with `ERR_PNPM_MISSING_TARBALL_INTEGRITY` [#13547](https://github.com/pnpm/pnpm/issues/13547).

- The lockfile's `time:` section is no longer dropped when `pnpm-lock.yaml` is rewritten. `resolutionMode: time-based` records each direct dependency's publish date there and now reads it back as the fallback for a package whose registry metadata carries no publish date, so a later resolution derives the same cutoff instead of picking different subdependency versions [#13776](https://github.com/pnpm/pnpm/issues/13776).

- Fixed pnpm failing to read `.modules.yaml` files containing long dependency paths [#13875](https://github.com/pnpm/pnpm/issues/13875). The manifest is now parsed as JSON (the format pnpm writes it in), falling back to the YAML parser only for manifests written by old pnpm versions.

- Fixed writing lockfiles with dependency paths longer than 1024 characters (long peer suffixes in large workspaces): such keys are now emitted in explicit `? <key>` form, matching the TypeScript CLI. Inline keys of that length are invalid YAML, so pnpm could not re-read the lockfile it had just written and every subsequent install re-resolved from scratch.

- `pnpm install` again records immature versions picked under `minimumReleaseAge` (when `minimumReleaseAgeStrict` is off) in `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`, so a later frozen install of the same lockfile passes verification [#13687](https://github.com/pnpm/pnpm/issues/13687).

- `resolutionMode` is no longer ignored when `minimumReleaseAge` is in effect. `lowest-direct` and `time-based` pick the lowest satisfying version of a direct dependency again; previously any active release-age cutoff — including the built-in default — silently forced the highest, so `resolutionMode` only worked when `minimumReleaseAge: 0` was set explicitly [#13752](https://github.com/pnpm/pnpm/issues/13752).

- Fixed `pnpm install` dropping a package that ships no `package.json` of its own from the lockfile. Such a package is now named after its alias and recorded at version `0.0.0` under `packages:` and `snapshots:`, and its extraction gets the placeholder `package.json` pnpm writes [#13410](https://github.com/pnpm/pnpm/issues/13410).

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a branch lockfile predates the removal of a dependency, or its move to another dependency group [#13966](https://github.com/pnpm/pnpm/issues/13966). A dependency that no project declares anymore is no longer reinstated by the merge, and the packages it was the only path to are dropped with it.

- `--config.minimum-release-age` is honored again, along with `--config.minimum-release-age-exclude`, `--config.minimum-release-age-ignore-missing-time` and `--config.minimum-release-age-strict`. Each overrides the matching `pnpm-workspace.yaml` setting, and the exclude flag may be repeated to build a list [#13929](https://github.com/pnpm/pnpm/issues/13929).

- Prevented `minimumReleaseAge` from replacing `latest` with a SemVer-greater version than the registry tag target [#13034](https://github.com/pnpm/pnpm/issues/13034).

- Reduced peak install memory: cached registry metadata is now read on demand from the on-disk metadata cache instead of being held in memory for the whole resolution. Resolving a large peer-heavy graph (`@teambit/bit`) peaks at about 1.3 GB instead of 3.2 GB, and a full cold install of it stays under 2 GB [#13681](https://github.com/pnpm/pnpm/issues/13681).

- `node_modules/.modules.yaml` now records `packageManager` as `pnpm@<release version>` (for example `pnpm@12.0.0-alpha.13`), matching `pnpm --version` and the TypeScript CLI. It previously recorded the internal crate name and crate version, `pacquet@0.0.1`.

- An unreadable `node_modules/.modules.yaml` no longer makes `pnpm install` delete `node_modules` and relink every package on each run. The unparsable state file is now reported as an error instead [#14062](https://github.com/pnpm/pnpm/issues/14062).

- Resolve optional peers from versions provided by local workspace packages, omit empty deprecation messages from generated lockfiles, and preserve valid lockfile pins in `pnpm dedupe --check`.

- Installs driven through `@pnpm/napi` got three fixes for large workspaces: the `readPackage` hook is now dispatched to JavaScript in batches instead of one event-loop roundtrip per manifest, the `dedupePeers` setting can be passed through the install options (so an existing lockfile generated with it is no longer treated as outdated), and version-pinned dependencies are served from the metadata mirror without queueing behind concurrent registry refreshes of the same package.

- Fixed the `overrides` block of `pnpm-lock.yaml` being rewritten in a random order on every install performed through `@pnpm/napi`. The recorded overrides now keep the order they were declared in, so repeat installs no longer churn the lockfile.

- The `TRACE` environment variable now enables engine tracing for `@pnpm/napi` consumers the same way it does for the pnpm CLI, and an invalid `TRACE` filter no longer aborts the process — it prints a warning and leaves tracing off.

- Aligned license reports, `dedupe --check` progress and spacing, and dependency-verification output with pnpm.

- A `file:` dependency declared by a package that was itself installed from a local directory is now resolved relative to that package's directory, not to the importer's [#13323](https://github.com/pnpm/pnpm/issues/13323). Installing a project whose local dependency depends on a sibling directory (`file:../child`) no longer fails with `Could not install from "…" as it does not exist`, and the snapshot entry for such a dependency is now written as `file:<path>` instead of `<name>@file:<path>`, matching the lockfile pnpm writes.

- Adding a package to a workspace no longer forces a full re-resolution when every dependency it declares is already locked for a sibling. The lockfile update writes the new project's importer entry from the versions the lockfile already holds; a dependency no locked version satisfies still reaches the resolver [#13696](https://github.com/pnpm/pnpm/issues/13696).

- Fixed peer resolution creating far more peer variants than the TypeScript CLI in multi-importer workspaces: a dependency subtree first resolved under one importer no longer hands the peer providers it resolved to every other importer that shares it. Those importers now bind such peers against their own context (or the workspace root), matching the TypeScript resolver. In a large bit.cloud workspace this cut a from-scratch install from 25,534 to 20,791 lockfile snapshots.

- `pnpm install --no-runtime` now works without `--frozen-lockfile`: on a fresh install, runtime dependencies are resolved and recorded in the lockfile, but their archives are not downloaded and their bins are not linked.

- Fixed `pnpm install` rewriting unrelated `pnpm-lock.yaml` entries after a small manifest change — for example, removing one dev dependency could bump other packages' open-range dependencies (such as jest's `@types/node: '*'`) to their newest versions [pnpm/pnpm#13193](https://github.com/pnpm/pnpm/pull/13193). Three resolution-reuse gaps caused still-satisfied lockfile entries to be re-resolved from the registry:

  - Direct dependencies using the `catalog:` protocol were compared against the lockfile in their resolved-range form, so every catalog-managed dependency looked changed on every install, and any package depending on one was re-resolved.
  - Auto-installed (hoisted) peer dependencies were also treated as changed direct dependencies on every install.
  - When a package had to resolve freshly but landed on the version the lockfile already recorded, its dependency subtree was still re-resolved instead of being reused, drifting open ranges pinned by the lockfile.

- Fixed empty `bundledDependencies` and `bundleDependencies` arrays causing nondeterministic lockfile changes. See pnpm/pnpm#13123.

- `npm_config_user_agent` now carries the configured user agent (`pnpm/<version> …`) in install lifecycle scripts, `pnpm run`, `pnpm exec`, and `pnpm dlx` [#13322](https://github.com/pnpm/pnpm/issues/13322). It was previously unset for install scripts and the bare string `pnpm` elsewhere, which made `preinstall` guards that check for pnpm reject the install.

- Lockfile verification now honors offline mode by using cached registry metadata instead of reaching the registry. When the required metadata is not available locally, verification reports the same `ERR_PNPM_NO_OFFLINE_META` condition used by offline resolution.

- An auto-installed *optional* peer is no longer hoisted at a version the workspace root's own dependency on that package excludes. `resolvePeersFromWorkspaceRoot` already made the workspace root's specifier decide which version a missing *required* peer is installed at; the optional-peer picker ignored it and always took the highest version present anywhere in the graph. In a workspace whose root pins `postcss: 8.5.10`, an importer that depends on `webpack` and declares no `postcss` of its own got `postcss@8.5.22` hoisted for `terser-webpack-plugin`'s optional `postcss` peer, leaving two `postcss@8.5.x` instances in the graph [#13320](https://github.com/pnpm/pnpm/issues/13320).

- A missing optional peer dependency is no longer satisfied by a prerelease version that its declared range doesn't accept. `ts-jest`, which declares `@jest/transform` and `jest-util` as optional peers with `^29.0.0 || ^30.0.0`, was bound to `30.0.0-alpha.6` when a `jest` 30 prerelease was elsewhere in the graph, while `jest` itself stayed on 29.

- With `autoInstallPeers: false`, a package's own optional peer dependencies are no longer added to its importer entry in `pnpm-lock.yaml` (and no longer linked into its `node_modules`) when another workspace project happens to resolve a matching version [#13325](https://github.com/pnpm/pnpm/issues/13325).

- Fresh installs no longer download the tarballs of platform-specific optional dependencies that don't match the current platform.

- Reduced registry metadata requests during dependency resolution by reusing cached metadata when lockfile preferences prove that no uncached version can win [pnpm/pnpm#13976](https://github.com/pnpm/pnpm/issues/13976).

- Fixed workspace lifecycle ordering and bin linking across isolated and hoisted installs.

- `pnpm outdated` and `pnpm update --interactive` now leave out the dependencies listed in `updateConfig.ignoreDependencies`, instead of reporting them and offering them for update.

- `pnpm outdated` and `pnpm update --interactive` now dereference `catalog:` specifiers before querying the registry. A catalog entry that is an npm alias (`'@types/zkochan__table': npm:@types/table@6.3.2`) no longer fails with `ERR_PNPM_OUTDATED_REGISTRY_ERROR` for the alias key, and `pnpm outdated --compatible` compares against the range the catalog holds instead of skipping the dependency.

- Fixed `pnpm outdated` and `pnpm update --interactive` offering versions blocked by `minimumReleaseAge` [pnpm/pnpm#14004](https://github.com/pnpm/pnpm/issues/14004).

- `pnpm outdated` now aligns its table borders when the output is colorized. The color escape codes in the `Package` and `Latest` cells were being counted as visible characters, so the columns and box-drawing borders drifted out of alignment on a terminal.

- Improved install performance: the store-index writer's shutdown now overlaps the install's final lockfile and `.modules.yaml` writes instead of extending the install's tail.

- Changing a `pnpm.overrides` entry to a version range now updates the lockfile in place when a version the lockfile already holds satisfies the range, instead of re-resolving the whole dependency graph. Only exact versions were handled before [#13696](https://github.com/pnpm/pnpm/issues/13696).

- `overrides` now also govern peers that pnpm auto-installs. Previously an override only rewrote dependencies declared in a manifest, so a peer nobody declares — installed because `autoInstallPeers` is on — resolved against its declared peer range and could bring in a second copy of the very package the override pinned. For example, with `overrides: { react: npm:react@19.2.0 }` and a lone `lucide-react` dependency, pnpm installed `react@18.3.1`; it now installs the pinned `react@19.2.0` [#13320](https://github.com/pnpm/pnpm/issues/13320).

- `pnpm pack` writes tar entries in the POSIX ustar header form npm uses — `ustar\0` magic and the explicit `0` regular-file typeflag — instead of the GNU form with a NUL typeflag, which strict tar readers such as publint mistake for the end-of-archive marker [#13924](https://github.com/pnpm/pnpm/issues/13924).

- `pnpm pack` and `pnpm publish` no longer let workspace-root `.gitignore` / `.npmignore` rules exclude files matched by the package manifest's `files` allowlist. Workspace packages whose build output is gitignored at the workspace root (for example a compiled `lib/` directory listed in `files`) were published with almost all payload files missing [#13164](https://github.com/pnpm/pnpm/issues/13164).

- A failed packument request now reports the status the registry returned (`404 Not Found`) instead of "error decoding response body".

- Fixed `pacquet add` to accept and install multiple package selectors in one operation.

- `pnpm approve-builds -g` is accepted again, reporting that the command is not supported with global packages rather than failing with `unexpected argument '-g' found`. `approve-builds` was the only command that declared `--global` without its `-g` short form [#13310](https://github.com/pnpm/pnpm/issues/13310).

- Auto-installed peer dependencies wanted by multiple packages under distinct but compatible ranges now resolve through the ranges' semver intersection (`2` + `^2.2.0` install one provider matching `>=2.2.0 <3.0.0-0`), matching pnpm. Previously such peers were only auto-installed when every consumer declared the identical range or `autoInstallPeersFromHighestMatch` was enabled.

- `pnpm pack` and `pnpm publish` now apply the `beforePacking` pnpmfile hook to the manifest before a package is packed, matching the TypeScript CLI.

- The Rust implementation of pnpm has moved from alpha to beta releases.

- POSIX shell shims now follow symbolic links before computing `basedir`, preventing execution failures when a shim is invoked via an external symlink on `PATH` [#13405](https://github.com/pnpm/pnpm/issues/13405).

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` now move a catalog entry's resolution to the requested version. Previously, when the catalog entry was a range that covered the requested version but resolved to a different one, the request was dropped silently: nothing was installed, nothing was written, and no error was raised.

- A catalog name containing a control character no longer corrupts `pnpm-workspace.yaml`. `pnpm add --save-catalog-name "$(printf 'a\nb')"` (or the same value in `saveCatalogName`) now fails with `ERR_PNPM_WORKSPACE_MANIFEST_WRITER_INVALID_CONTROL_CHARACTER` and leaves the file untouched, matching how the writer already treats `allowBuilds` and `overrides` entries.

- The `--help` text now reads as user-facing help rather than developer documentation. Command and flag descriptions say what each option does for you, and the leftover markdown that was printing verbatim in the terminal — intra-doc links, an inline link, and an HTML-like path placeholder — has been cleaned out.

- Improved `pnpm add` performance for multiple package selectors by resolving them concurrently [pnpm/pnpm#13089](https://github.com/pnpm/pnpm/issues/13089).

- Fixed `--config.ignore-scripts=true` not being honored by CLI commands such as `pnpm pack` [#13986](https://github.com/pnpm/pnpm/issues/13986).

- Preserve each direct dependency's locked optional peer context during `pnpm dedupe`.

- Preserve optional peer providers recorded in peer suffixes when `pnpm dedupe` rebuilds a workspace lockfile.

- `engineStrict` now fails the install when an incompatible package is reached through a regular dependency edge of an installable package, even if the package is also optionally reachable — matching pnpm. Packages reachable only through optional edges or skipped parents are still skipped [#13143](https://github.com/pnpm/pnpm/issues/13143).

- Engine checks (`engines.node` / `engines.pnpm`) now match npm-semver's `includePrerelease` semantics exactly: a prerelease version no longer satisfies a fully specified `>=` bound (`9.0.0-alpha.1` does not satisfy `>=9.0.0`), while still satisfying expanded ranges like `9`, `>=9`, and `^9.0.0`.

- Every error code is now an `ERR_PNPM_*` code, matching the codes pnpm has always used. Errors previously reported internal Rust-crate codes such as `pacquet_package_manager::outdated_lockfile` or unprefixed codes such as `GIT_CHECKOUT_FAILED`; these are now `ERR_PNPM_OUTDATED_LOCKFILE` and `ERR_PNPM_GIT_CHECKOUT_FAILED`. Where pnpm defines a code for the same error, pnpm's exact code is used. Scripts and CI that match on the old codes need updating.

- Fixed fresh isolated installs to enforce incompatible required dependency engines when `engineStrict` is enabled.

- Fixed `pnpm install --frozen-lockfile` incorrectly failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a workspace project declares `peerDependencies` that `auto-install-peers` resolves. With `auto-install-peers` enabled (the default), pnpm records those missing peers in the lockfile importer's `dependencies`; the frozen-lockfile freshness check now folds `peerDependencies` into the comparison instead of reporting the materialized peers as removed.

- A setting in the global `config.yaml` that pnpm does not read from that file, or that is written in kebab-case instead of camelCase, is now reported instead of being ignored silently.

- Fixed two global-virtual-store correctness gaps. A failed build now discards the hash directory it was building in, so the next install re-fetches instead of reusing a half-built directory shared by every project with the same dependency graph. The removal only ever touches a slot strictly inside the store, so a crafted package name cannot make it escape. And a side-effects-cache hit no longer assumes the store slot still holds the cached build: when the slot has been re-imported pristine, the build output is materialized rather than skipped, which previously left the package without its build artifacts.

  `.modules.yaml` now records the `allowBuilds` set the install ran under, matching pnpm.

- With `nodeLinker: hoisted`, a workspace project no longer gets its own copy of a dependency whose version already won the workspace-root slot. Only the versions that lost the root slot are nested, matching the pnpm CLI. Previously every project's direct dependency was materialized under that project as well, which gave lifecycle scripts a second copy to run in.

- Resolve `catalog:` specifiers in the dependencies of injected workspace packages (`injectWorkspacePackages: true`). Previously such a child spec bypassed catalog resolution and failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`, matching the TypeScript CLI.

- Fixed Pacquet workspace commands to honor project filters, preserve complete lockfile state, and materialize only the selected dependency closure, including pnpr-backed installs.

- `pnpm install <pkg>` now adds the package, the same as `pnpm add <pkg>` and matching the JavaScript CLI. It previously ended in a usage error: `pnpm i valibot` printed `error: unexpected argument 'valibot' found` instead of saving the dependency [#13886](https://github.com/pnpm/pnpm/issues/13886).

- The Rust engine now warns when `package.json` still declares install settings under the `pnpm` field, which pnpm 10 moved to `pnpm-workspace.yaml`. A project that hasn't migrated its `pnpm.overrides` / `pnpm.packageExtensions` / `pnpm.patchedDependencies` previously saw the settings silently ignored, and only met the downstream symptom. Keys the `pnpm` field never owned are left alone.

- Fixed `pnpm licenses list` and `pnpm licenses ls` parsing and license metadata discovery when using the global virtual store [pnpm/pnpm#13332](https://github.com/pnpm/pnpm/issues/13332) and [pnpm/pnpm#13333](https://github.com/pnpm/pnpm/issues/13333).

- `pnpm list --json` and `pnpm list --parseable` now report extraneous packages — packages present in `node_modules` but absent from the lockfile — under `unsavedDependencies`, matching the TypeScript CLI.

- `pnpm login` / `pnpm adduser` now read the `scope` setting from `pnpm-workspace.yaml`, the global `config.yaml`, and the `PNPM_CONFIG_SCOPE` environment variable, not only from the `--scope` command-line flag. When `scope` is configured, the granted token is keyed to that scope and the scope-to-registry mapping is recorded. `--scope` still takes precedence when both are set. Note that `scope` in an `.npmrc` is not read — pnpm keeps only auth and registry keys from that file.

- `pnpm add <pkg>` (without a version) and `pnpm update --latest` now resolve the `latest` dist-tag through the `minimumReleaseAge`-aware picker, pinning the newest version that satisfies the cutoff instead of writing a range the follow-up install rejects. An invalid `minimumReleaseAgeExclude` value reported by these commands now carries the same `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE` error code the install reports. See pnpm/pnpm#11165.

- A `pnpm-workspace.yaml` that declares a package pattern whose directory does not exist yet — `packages/*` before the first package is created, say — no longer fails every command with `ERR_PNPM_WORKSPACE_WALK_ERROR`. The pattern now matches no projects, as it does in the JavaScript implementation [#13296](https://github.com/pnpm/pnpm/issues/13296).

- Fixed Plug'n'Play projects to preload `.pnp.cjs` for dependency and project lifecycle scripts, `pnpm run`, and `pnpm exec`. The generated loader now also exposes the public Yarn PnP API surface.

- Fixed `catalog:` references failing to resolve when installing through a pnpr server, which errored with "No catalog entry '<name>' was found for catalog 'default'." even though the catalog entry existed. The workspace the server reconstructs from the request has no catalog sections, so the client now sends its catalogs along with the request [#13232](https://github.com/pnpm/pnpm/issues/13232).

- Validate the project's pinned package manager and runtimes before running a command, matching the pnpm CLI:

  - A `packageManager` / `devEngines.packageManager` pin that the running pnpm does not satisfy now fails with `ERR_PNPM_BAD_PM_VERSION` (or `ERR_PNPM_OTHER_PM_EXPECTED` when the project is pinned to another package manager), instead of being silently ignored. The check also runs under corepack, where pnpm cannot switch versions itself, and says so.
  - `devEngines.runtime` / `engines.runtime` entries with `onFail: "error"` or `onFail: "warn"` are validated against the Node.js, Deno, or Bun installed on the system, failing with `ERR_PNPM_BAD_RUNTIME_VERSION`.
  - `pmOnFail` and `runtimeOnFail` are honored as bypasses and can now be passed as `--pm-on-fail=<value>` / `--runtime-on-fail=<value>`, the form the error hints suggest.

  Global commands (`--global`) and commands that do not belong to the project (`store`, `dlx`, `self-update`, …) skip these checks, as does a project pin that only asked pnpm to switch versions when `manage-package-manager-versions` is turned off.

- Arguments after the script or command name now reach the script untouched for `pnpm run`, `pnpm exec`, `pnpm dlx`, and `pnpm with`, matching the JavaScript implementation. Previously `pnpm run build --config.foo=bar` consumed the argument as a pnpm setting instead of forwarding it, and `pnpm run build --silent` handed the script `--reporter=silent` — a token the user never typed [#13302](https://github.com/pnpm/pnpm/issues/13302). Put such flags before the script name (`pnpm run --silent build`) to apply them to pnpm.

- Fixed generated lockfiles to preserve packages' scalar `libc` constraints.

- Preserve a user-provided `TMPDIR` when scripts run with `unsafePerm` enabled; otherwise, continue using the package-local temporary directory.

- Aligned large-download progress byte formatting with pnpm.

- Added support for `publishConfig.name`, which publishes a package under a different name than the one its manifest carries in the workspace. Only the published artifact is renamed — dependents, `pnpm-lock.yaml`, and release tooling keep addressing the project by its manifest name — and the new name reaches the packed manifest, the tarball filename, and everything that addresses the package at the registry: the already-published check of `pnpm publish -r`, its registry selection, and the release-planning probes of `pnpm change status` and `pnpm version -r`. This also fixes the changelog of the Rust CLI itself, which is published as `pnpm` from a workspace project named `pacquet`: its release notes were composed under the workspace name and so never made it into the published package [#13345](https://github.com/pnpm/pnpm/issues/13345).

- Close three CLI parity gaps with the TypeScript pnpm CLI:

  - `--registry <url>` is now accepted on every command as a universal rc-option, not only through `--config.registry=<url>` (`pnpm view pnpm dist-tags.latest --registry=https://registry.npmjs.org/`).
  - `pnpm add` (and `pnpm add -g`) now accept `--allow-build=<pkg>`, appending the named packages to `allowBuilds` so they can run their lifecycle scripts during the install (`pnpm add @pnpm/exe@11.16.0 --allow-build=@pnpm/exe`).
  - `--dir` / `-C` is now position-independent: it is accepted anywhere on the command line, before or after the subcommand (`pnpm add foo --dir /tmp/proj`).

- Fixed duplicate package statistics output during installs in non-interactive terminals.

- `pnpm run <script> <args>` now forwards every argument after the script name to the script verbatim, matching the behavior of the JavaScript implementation. Previously the `--` separator was dropped, so `pnpm run test -- --watch` reached the underlying program as `--watch` and failed whenever that program claimed the option itself; arguments spelled like `pnpm run`'s own flags (`-s`, `--if-present`) were also consumed by pnpm instead of reaching the script [#13295](https://github.com/pnpm/pnpm/issues/13295). Pass those flags before the script name (`pnpm run -s test`) to apply them to pnpm.

- Fixed installs failing on Windows when a scoped dependency (`@scope/name`) had to be symlinked. Its `node_modules/@scope/name` link path was built by joining the whole alias as one segment, which left a `/` in the otherwise `\`-separated path; that forward slash reached `CreateSymbolicLinkW`, which rejects forward-slash paths with `ERROR_DIRECTORY` (os error 267). Paths are now rewritten to native separators before every filesystem call in the symlink writer.

- `pnpm test`, `pnpm start`, and `pnpm stop` now forward their arguments to the script, matching the pnpm CLI. `pnpm test --watch` and `pnpm start --port 3000` previously failed with a usage error, and `pnpm stop` claimed `--if-present` and `-s` for itself instead of passing them on. As with `pnpm run`, every token after the command name reaches the script verbatim, a `--` separator included.

- Fix `pnpm self-update <dist-tag>` recording the dist-tag (e.g. `next-12`) as the `packageManagerDependencies` specifier in `pnpm-lock.yaml`. It now records the resolved `devEngines.packageManager` pin, matching the manifest, so a later `--frozen-lockfile` install no longer fails with "the lockfile is not up to date".

- Prompt before installing packages that do not meet a strict `minimumReleaseAge`, persist approved versions to `minimumReleaseAgeExclude`, and keep progress output from overwriting the prompt.

- Fixed installs failing on Windows with `ERROR_DIRECTORY` (os error 267) when re-linking over a global store whose directory junctions were restored in a dangling state (for example, a `store/v11` directory brought back by a CI cache, since tar can't round-trip a Windows reparse point). `create_dir_all` accepts such a junction because it keeps the directory attribute, but `CreateSymbolicLinkW` can't create a child link through it. The symlink writer now rebuilds the broken parent directory and retries instead of aborting the install.

- Error messages and `--help` text now refer to the CLI as `pnpm` instead of the internal `pacquet` name. Several messages previously suggested commands like `pacquet install --frozen-lockfile`, which is not a command users can run, and `pnpm add --help` documented the virtual store directory default as `node_modules/.pacquet` rather than the actual `node_modules/.pnpm`.

- Fixed `workspace:` dependencies failing to resolve when they point at a named workspace package whose `package.json` has no `version` (or a `null` version). Such packages are now indexed as version `0.0.0`, matching the TypeScript CLI, so specs like `workspace:*` and `workspace:0.0.0` resolve instead of failing with a misleading "no package named" error.

- Fixed installs to detect manifest changes in workspace members and reject stale lockfiles when using `--frozen-lockfile` [pnpm/pnpm#13080](https://github.com/pnpm/pnpm/issues/13080).

- Added the `--workspace-root` (`-w`) flag, which runs the command on the root workspace project. `pnpm add -D typescript prettier -w` from a workspace subdirectory now saves to the root `package.json` instead of failing with "unexpected argument '-w' found" [#13031](https://github.com/pnpm/pnpm/issues/13031). Combined with `--recursive`, the flag narrows the run to the root project alone. `-w` may not be used together with `--global`, and may only be used inside a workspace.

- Workspace packages declared with a parent-relative pattern in `pnpm-workspace.yaml` (`../shared`, `../../docs/*`) are discovered again. They were dropped from the project list, so `pnpm list -r` and `--filter` did not see them and a frozen install of a lockfile that already held their importer entries failed with `ERR_PNPM_PACKAGE_MANAGER_UNSAFE_IMPORTER_PATH`.

- Changing a parent-scoped `pnpm.overrides` entry (`"parent>child": "2.0.0"`) now updates the lockfile in place instead of re-resolving the whole dependency graph. Only the named parent's dependency moves; every other package keeps the version it had [#13795](https://github.com/pnpm/pnpm/issues/13795).

- `patchedDependencies` patch files that pnpm applies no longer fail with `ERR_PNPM_PATCH_FAILED`: a hunk whose last line is context in a file with no final newline, and an LF patch against a CRLF file, both apply again [#13322](https://github.com/pnpm/pnpm/issues/13322). A hunk that has drifted from its recorded line numbers is also retried nearby, matching pnpm.

- Fixed non-deterministic lockfiles on cold installs of projects with cyclic peer dependencies: resolved peer variants could silently drop from the lockfile depending on traversal order [#13846](https://github.com/pnpm/pnpm/issues/13846), [#13865](https://github.com/pnpm/pnpm/issues/13865).

- Resolution spends less time in its final peer pass: the package-name cycle graph it consults is now derived once per package instead of once per occurrence of that package.

- A peer dependency is now recorded in the lockfile at the version and peer suffix the peer provider actually resolved to. Peers whose provider carried peer suffixes of its own could be recorded against a package instance that no importer installs, leaving an unreachable entry in `snapshots:` and a peer bound to the wrong instance [#13320](https://github.com/pnpm/pnpm/issues/13320).

- A peer dependency that the workspace root already provides is no longer installed a second time. With `resolvePeersFromWorkspaceRoot` enabled (the default), a missing peer is matched against the **workspace root** project's dependencies; it was matched against the dependencies of whichever project was being resolved, so a project that didn't declare the peer itself resolved its own copy from the registry. In [vercel/next.js](https://github.com/vercel/next.js), whose `overrides` pin `react` to a single canary build, this pulled in a second `react` and paired it with `react-dom` from the canary — a combination the pin exists to prevent.

- Under `resolvePeersFromWorkspaceRoot`, a workspace root dependency declared with `link:` or `file:` (or the path form of `workspace:`, such as `workspace:../pkg`) now satisfies another project's missing peer dependency at the linked package's own version, instead of being hoisted as a path. Those specifiers are relative to the project that declares them, so the same specifier reached a different directory — or none — from the project the peer was hoisted into, leaving a broken link. The root now has the same authority over the peer as it has when it declares the package with a version range [#13373](https://github.com/pnpm/pnpm/issues/13373).

- Reduced peak memory usage while resolving peer dependencies. Workspaces with large, deeply peer-dependent dependency graphs could need gigabytes to install; the same install now needs meaningfully less.

- Two `pnpm install` peer-resolution fixes that made large workspaces such as [Astro](https://github.com/withastro/astro) produce a different `pnpm-lock.yaml` than pnpm 11 [#13334](https://github.com/pnpm/pnpm/issues/13334):

  - A package that declares the same name in both `dependencies` and `peerDependencies` no longer gets a nested copy of it when the parent already supplies that name, which is what pnpm does with `autoInstallPeers` disabled. The nested copy hid the peer, so the package was recorded without the peer context it resolves in.
  - A duplicate peer-suffixed variant that collapses into a larger, compatible one now collapses everywhere it is referenced. A variant kept alive by a single consumer's edge no longer lingers in the lockfile.

- Removing a dependency, or moving one to another already-locked version, no longer re-resolves the whole dependency graph just because some package resolves a peer with the same name. The lockfile update now compares the peer suffixes against the exact `name@version` the removal severed, so a suffix that names a different — still present — version of that dependency is left alone [#13781](https://github.com/pnpm/pnpm/issues/13781).

- Reduced the peer resolution pass's CPU cost on workspaces with many peer dependencies. The walker cloned its parent peer-context maps at every node — twice per node plus once per child — even when a node contributed nothing to them; the maps are now shared copy-on-write and the derived per-child snapshots are reused unless the context actually changed. On a peer-heavy 331-importer benchmark the full resolution dropped from 3.9 s to 2.8 s.

- Installs with a cold cache are significantly faster: lockfile verification no longer delays resolution or downloads and re-checks far less data over the network, and downloaded packages are linked while the remaining downloads are still in flight.

- Dependency resolution is faster: package metadata is now filtered once per packument instead of once per dependency edge when `minimumReleaseAge` is active, and parsed semver versions and ranges are reused instead of re-parsed on every comparison.

- Closed the remaining gaps in how unscoped per-registry `.npmrc` settings are pinned to the registry their own source file declared:

  - An inline `cert=` / `key=` written with `\n` escapes now expands to a real multi-line PEM, matching the URL-scoped `//host/:cert=` spelling.
  - `pnpm config get` / `pnpm config list` now report a rescoped credential under the URL-scoped key it was pinned to, instead of the unscoped key it was written as.
  - The deprecation warning names the file it read and lists every setting it pinned, including `tokenHelper`.
  - A credential with no registry of its own is no longer attached to the resolved default registry, which repository config can move. The same rule now covers the `@pnpm/napi` bindings: the `authHeaderByUri` entry written with an empty (`""`) key is pinned to the `registry` / `registries.default` the host passed alongside it, never to a registry the project's `.npmrc` names.

- `pnpm install` no longer re-resolves dependencies inside a subtree the lockfile pinned when another dependency reaches the same package. Those packages kept their locked versions in `node_modules` while `pnpm-lock.yaml` recorded newer ones, so an install could quietly move a transitive dependency — including across a major version — without anything asking it to.

- `pnpm pkg get` and `pnpm pkg set` now accept hyphens inside a dot-notation property path, so `pnpm pkg get dependencies.some-package-name` reads the key instead of failing with `ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH`. The bracketed and quoted forms already worked and are unchanged.

- The automatic `packageManager` version switch works again on registries whose tarball URLs point at a different host than the registry itself (load-balanced feed proxies, Artifactory-style mirrors). Package-manager entries are now always recorded with integrity-only resolutions — the download URL is derived from the trusted bootstrap registry instead — and entries persisted in an invalid shape by an earlier pnpm are discarded and re-resolved instead of failing every command [#13619](https://github.com/pnpm/pnpm/issues/13619).

- Registries that serve no npm signature metadata (private mirrors and feed proxies commonly strip `dist.signatures`) no longer break the automatic `packageManager` version switch and `pnpm self-update` [#13147](https://github.com/pnpm/pnpm/issues/13147). When the configured registry cannot provide a verifiable signature, pnpm now fetches the signature from `registry.npmjs.org` and verifies it against the same embedded npm keys over the installed integrity — which proves exactly the same thing. If no signature can be obtained from either source (for example, both are unreachable, or the registry publishes only a `shasum`), pnpm proceeds with a warning instead of failing, but only when the packages resolve through a registry configured in the user's own (non-project) configuration; the download stays pinned by the lockfile integrity, and a signature that exists but does not validate still fails the switch.

- `pnpm fetch`, and any install run with `virtualStoreOnly`, no longer writes a `.pnp.cjs` loader under `nodeLinker: pnp`. These installs populate the virtual store without linking the project, so the loader would have claimed the project resolves out of a store it was never linked into. The importer links and `node_modules/.package-map.json` were already skipped; the PnP loader now follows the same rule.

- A `.pnpmfile.cjs` `readPackage` hook that rewrites one of a project's *own* dependency specifiers is now honored: rewriting `"is-positive": "^1.0.0"` to `1.0.0` installs 1.0.0 and records `specifier: 1.0.0` for the importer. Previously the hook was applied only to the manifests of resolved dependencies, so a project's own specifier resolved against the raw range from `package.json` [#13769](https://github.com/pnpm/pnpm/issues/13769).

- A path named by the `pnpmfile` setting that is not on disk now fails with `ERR_PNPM_PNPMFILE_NOT_FOUND` and names the file, instead of surfacing as a generic pnpmfile execution failure. Discovery of the default `.pnpmfile.mjs` / `.pnpmfile.cjs` is unaffected: a project that ships neither still installs normally.

- Fixed `pnpm` installs using pnpr to honor the client's `autoInstallPeers`, `dedupePeers`, and `excludeLinksFromLockfile` settings [pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389).

- With a configured `pnprServer`, `pnpm install` skips the server exchanges it does not need, closing the gap where an up-to-date project paid a full resolve round trip that a direct install answered locally [pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904):

  - The repeat-install "Already up to date" fast path now runs with a pnpr server configured.
  - An install whose `pnpm-lock.yaml` still satisfies every manifest skips the server resolve exchange and materializes `node_modules` from the on-disk lockfile.
  - The input-lockfile verification round trip is skipped when the local `lockfile-verified.jsonl` cache already covers the lockfile under the current policy; server-verified and server-resolved lockfiles are now recorded into that cache.
  - Changing the `trustPolicy*`, `minimumReleaseAgeStrict`, or `minimumReleaseAgeExclude` settings now invalidates the repeat-install fast path, matching the TypeScript CLI's workspace-state check.

- The install summary no longer prints `(X is available)` when the registry's `dist-tags.latest` is still held back by the active `minimumReleaseAge` policy. The hint only ever names the actual latest tag, so an immature latest suppresses the hint instead of advertising the version pnpm just refused to install [#11698](https://github.com/pnpm/pnpm/issues/11698).

- npm's `--prefix` is accepted as a spelling of `--dir`, and `--store` as a spelling of `--store-dir`, so `pnpm --prefix ../ run test` no longer fails with "unexpected argument '--prefix' found" [#13583](https://github.com/pnpm/pnpm/issues/13583).

- `pnpm update` keeps the explicit `=` operator of an exact version pin: a dependency saved as `=3.5.1` now updates to `=3.5.2` instead of the bare `3.5.2`. See pnpm/pnpm#13168.

- A lockfile entry whose resolution is unchanged no longer loses its recorded `deprecated` marker when a registry serves the package's metadata inconsistently — re-resolving to the same version keeps the deprecation instead of silently dropping the line [#13846](https://github.com/pnpm/pnpm/issues/13846).

- Preserve a workspace dependency's `link:` entry when a run does not target it — e.g. `pnpm update <other-pkg>` (with or without `--recursive`), or a plain install after a root/catalog dependency change — with `injectWorkspacePackages`, instead of spuriously rewriting it to a peer-suffixed `file:` protocol. See pnpm/pnpm#10433.

- Fixed resolution of a direct dependency declared in both `dependencies` and `devDependencies`: the `dependencies` specifier now wins, matching the TypeScript CLI. The `devDependencies` range was resolved instead, recording a lockfile importer entry whose version did not satisfy its specifier — which failed the lockfile up-to-date check and forced a full re-resolve on every install.

- The projects that run their own lifecycle scripts (`preinstall`, `install`, `postinstall`, `prepare`, …) now match pnpm in every install-family command. A project runs them when the command installs it in full, and — in a workspace the command only partly covers — whenever the command mutates it at all; the workspace root runs them even when the command was pointed at another project, because it is installed in full alongside it. As a result, `pnpm update <pkg>` and `pnpm add <pkg>` in a workspace no longer skip the workspace root's scripts, `pnpm update` at a workspace root no longer runs the other members' scripts, and `pnpm update --latest` no longer runs the project's own scripts (it rewrites named dependency specs, so it is a partial install like `pnpm update <pkg>`) [#13358](https://github.com/pnpm/pnpm/issues/13358).

- Prevent pnpm from removing project files when `modulesDir` resolves to the project root.

- A forced full re-resolution (config changes the fast lockfile update cannot absorb, such as a changed override or `packageExtensions`) no longer moves dependencies whose recorded versions still satisfy their ranges. The prior lockfile now pins each still-satisfied edge even when its recorded subtree cannot be reused wholesale, so open ranges like `@types/node: "*"` keep their locked versions instead of collapsing onto the highest locked version and churning the lockfile.

- `pnpm remove` now prunes undecided entries (`"set this to true or false"`) from `allowBuilds` in `pnpm-workspace.yaml` when `sharedWorkspaceLockfile: true` and the corresponding packages are removed [pnpm/pnpm#13892](https://github.com/pnpm/pnpm/issues/13892).

- `pnpm prune` now prints the `Scope: all N workspace projects` line when run inside a workspace, as it prunes every project of the workspace.

- Removing a package from a workspace now drops its importer entry from `pnpm-lock.yaml`, along with the dependencies only it needed. Previously the entry survived every later install, which kept those dependencies reachable and made the lockfile diverge from the one the TypeScript CLI writes [#13783](https://github.com/pnpm/pnpm/issues/13783).

- `pnpm publish` again sends the package's README to the registry as metadata, so registries can render it on the package page. The readme is always included in the published metadata (matching the npm CLI), while the `embed-readme` setting continues to control only whether the readme is written into the `package.json` inside the tarball. This restores the behavior that was lost when publishing became fully native. Closes pnpm/pnpm#12966.

- Suggest `pnpm shim add <runtime>` after pinning a project runtime when no project-aware global shim is installed. Explicit project-aware shims now reject unrelated global bin conflicts and are restored after a matching global package is removed or replaced by a version that drops its bin.

- Speed up installs after adding `ignoredOptionalDependencies` patterns by removing newly ignored optional dependencies and pruning packages that are no longer reachable without resolving the dependency graph again.

- Improved fresh resolution performance when package metadata is already cached.

- Reduced memory use when resolving peer-heavy dependency graphs and prevented nested hoisted graphs from expanding into excessive dependency paths.

- Print the script command by default when running a filtered lifecycle script. The command remains hidden with `--silent`.

- Run dependency verification consistently after regenerating a lockfile with `dedupePeers` enabled.

- Kept the lockfile policy verdict ahead of the frozen-install message when package statistics arrive while verification is still running.

- Aligned the `hoistedDependencies` contents and ordering in `node_modules/.modules.yaml` with pnpm.

- Preserve whether package `libc` metadata uses a string or an array when writing the lockfile.

- A `package.json` that starts with a UTF-8 byte order mark is read again instead of failing with `expected value at line 1 column 1`. Workspace discovery, dependency manifests (including bin linking), tarball extraction, and `pnpm publish` of a pre-built tarball all accept one, matching pnpm [#13311](https://github.com/pnpm/pnpm/issues/13311). A manifest that really is malformed now reports its path in the error.

- Fixed `pnpm install` ignoring a `pnpm-lock.yaml` that carries a leading env lockfile document when the file has CRLF line endings or a UTF-8 byte order mark, as a `core.autocrlf` checkout on Windows produces. The lockfile was reported as broken with `multiple YAML documents detected` and every dependency was re-resolved from the registry [#13606](https://github.com/pnpm/pnpm/issues/13606).

- Fixed `ERR_PNPM_BROKEN_LOCKFILE` when installing with a pnpm 10 lockfile that has a `patchedDependencies` section. See pnpm/pnpm#13307.

- Record the pnpm version a project pins even when the install has nothing else to do. Adding a `devEngines.packageManager` (or `packageManager`) pin to a project whose dependencies are already installed left `packageManagerDependencies` unwritten, so `pnpm install --frozen-lockfile` failed with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` while a plain `pnpm install` reported "Already up to date" without recording it [#14124](https://github.com/pnpm/pnpm/issues/14124).

- Recover from a metadata cache entry that disappears (concurrent cache cleanup, antivirus) after the registry has already answered the conditional request with `304 Not Modified`. The metadata is re-requested once without cache validators instead of failing the install with `ERR_PNPM_CACHE_MISSING_AFTER_304`.

- `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm remove` now support recursive (`-r`) and filtered (`--filter`) execution in workspaces configured with one lockfile per project (`sharedWorkspaceLockfile: false`), instead of failing with `ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED`. Each selected project is installed independently against its own `pnpm-lock.yaml`, `node_modules`, and virtual store, matching pnpm.

- `pnpm -r run "/pattern/" --no-bail` no longer exits zero when one of a project's matched scripts fails and a later one passes. The run summary carries a single status per project, and the passing script overwrote the recorded failure.

- `pnpm -r update --latest --depth 0 <selector>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when no project in the workspace declares a matching dependency, instead of silently doing nothing.

- Changing `--os` / `--cpu` / `--libc` or `supportedArchitectures` between installs now re-evaluates previously skipped optional dependencies, so the platform packages for the newly selected architecture are installed instead of staying skipped.

- A project pinned to a broken pnpm release via `packageManager` or `devEngines.packageManager` now reports which release is broken and what to do about it, instead of failing inside the installer. `pnpm self-update` already refused these releases; the version switch does too.

- The three registry lookups are now named for what they are keyed by, so that none of them is called `registries` — a name the `registries` setting itself has taken:

  | before | after |
  |---|---|
  | `Config.registries` | `Config.registriesByScope` |
  | `Config.namedRegistries` | `Config.registriesByPrefix` |
  | `Config.registryOptions` | `Config.registryOptionsByUrl` |

  The same rename applies to the `RegistryContext` fields, the `Registries` and `NamedRegistries` types (now `RegistriesByScope` and `RegistriesByPrefix`), `normalizeRegistries` / `normalizeNamedRegistries` (now `normalizeRegistriesByScope` / `normalizeRegistriesByPrefix`), and the `BUILTIN_NAMED_REGISTRIES` constant (now `BUILTIN_REGISTRIES_BY_PREFIX`).

  This is an internal rename: no setting, error code, lockfile field, or `.pnpmfile.cjs` hook field changes. A `preResolution` hook still reads `ctx.registries`, which is the name pacquet passes as well. The `registries` and `namedRegistries` settings are read under the names users write them.

  The pnpr resolve request sends `registriesByPrefix` where it sent `namedRegistries`. A pnpr server and its clients must be on matching versions, which is already the case for an experimental server.

- `pnpm remove` no longer re-resolves the dependency graph. The removed dependency's entries are dropped from `pnpm-lock.yaml` and anything they made unreachable is pruned, without registry access. The install still falls back to a full resolution when a surviving package resolves a peer dependency through the removed one.

- An install sharing a global virtual store no longer removes an incomplete package directory that another importer is still writing, which could fail with `failed to remove existing directory ... prior to swap: Directory not empty`. Such a directory is now repaired in place, and a package file left damaged by an interrupted install is restored instead of being kept.

- Fixed the incremental install fast path wrongly reporting "already up to date" — skipping re-resolution — when a `package.json`, `.pnpmfile.cjs`, or patch file was edited immediately after an install. The freshness check compared file modification times against a wall-clock timestamp, which broke in two ways: on a machine whose wall clock and filesystem clock disagree (seen on some CI runners) the timestamp could sit ahead of a later edit's mtime, and a fast install could write its lockfile in the same millisecond as the subsequent edit. The check now records the baseline from filesystem mtimes and compares at nanosecond precision.

- Fixed the dependency status check wrongly reporting "up to date" when a `package.json`, `.pnpmfile.cjs`, or patch file was edited in the same second as the previous install, on filesystems that record mtimes at whole-second resolution (for example ext4 with 128-byte inodes). The optimistic repeat-install fast path and `verify-deps-before-run` compared mtimes strictly, so a same-second edit whose mtime rounded down looked unchanged and re-resolution was skipped. Such a file's whole second is now treated as possibly-modified, falling through to the content check; behavior on sub-second filesystems is unchanged.

- Fixed repeat installs paying for a full lockfile comparison forever after a modification-time collision. When a `package.json` was last modified inside the same clock tick that the install recorded as its validation baseline — a fast install, a checkout that copied files with identical timestamps, or any filesystem that keeps only whole-second modification times — the manifest kept reading as possibly-modified, so every later `pnpm install` and `verify-deps-before-run` check re-compared the manifests against the lockfile instead of taking the fast path [#13907](https://github.com/pnpm/pnpm/issues/13907).

- Resolution failures now report the error pnpm defines for them. A well-formed range that the registry publishes nothing for fails with `ERR_PNPM_NO_MATCHING_VERSION` — naming the latest release, the other dist-tags, and the `pnpm view <pkg> versions` command that lists the rest — instead of `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`. A package the registry doesn't have fails with `ERR_PNPM_FETCH_404` and the "not in the npm registry, or you have no permission to fetch it" hint (plus which authorization header was sent, since a private registry often answers a permission failure with a 404) instead of a bare HTTP-client message. A wrapper that quotes its cause verbatim no longer prints the same sentence twice in the error report.

- An install that had to re-hash store files to verify them now reports it. If that cost more than a second, it says how long — `The integrity of N files was checked in 2.5s.` — and if it was quick but covered more than a thousand files, it names the cause instead: their timestamps changed since the store recorded them, which a backup tool, an antivirus scan or a copied store can do.

- Fixed `pnpm add <workspace-package>` to resolve the local package when `linkWorkspacePackages` is enabled.

- `$dep-name` self-references in `overrides` are now resolved against the root manifest's direct dependencies, so an override such as `rolldown: $rolldown` records the concrete specifier in `pnpm-lock.yaml` and no longer fails a frozen install with `ERR_PNPM_OUTDATED_LOCKFILE` [#13314](https://github.com/pnpm/pnpm/issues/13314). A reference to a package that is not a direct dependency fails with `ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION`, and the deprecated syntax now warns, pointing at catalogs.

- `overrides` are now applied after the `readPackage` hook during resolution, matching the TypeScript CLI's hook order (`packageExtensions` → `readPackage` hooks → `overrides`). A hook that replaced a manifest — such as a host application substituting a workspace project's raw manifest for its injected instances — previously erased the overrides from that manifest, so the resolved graph ignored them.

- Retry package metadata requests when a registry or proxy returns `304 Not Modified` to an unconditional request, preventing false `ERR_PNPM_CACHE_MISSING_AFTER_304` failures [pnpm/pnpm#12882](https://github.com/pnpm/pnpm/issues/12882).

  If the retry also returns `304`, report `ERR_PNPM_META_NOT_MODIFIED_WITHOUT_CACHE` instead.

- `pnpm publish --provenance` now applies the `fetch-timeout` setting to the sigstore signing exchange and retries it up to two more times with exponential backoff when it fails or times out, instead of aborting the publish on the first transient network error or hanging on a stalled connection.

- Fixed a lockfile corruption during non-frozen re-installs: when one workspace project reused a package's resolution from the lockfile and another project's edge to the same package was denied reuse (for example because it also depends on a direct dependency whose specifier changed), the denied edge could read the reused, dependency-less resolution from the shared wanted-dependency cache and record the package as a leaf. Its lockfile snapshot became empty (`{}`), its peer suffix was dropped, and none of its dependencies were linked, which later broke installs and builds consuming that lockfile [#13070](https://github.com/pnpm/pnpm/pull/13070).

- Improved fresh installs by reusing the store index and verified-files cache during dependency materialization.

- Removing a package from `allowBuilds` now fails the next `pnpm install` under `strictDepBuilds` instead of reporting the project as already up to date. A build whose output is already cached in the store no longer counts as an approval [#11035](https://github.com/pnpm/pnpm/issues/11035).

- Under `nodeLinker: isolated`, a Bit root-component member whose materialized copy carries no `package.json` now receives sibling symlinks for the dependencies its own lockfile snapshot declares, instead of a symlink to every other member of the root. The all-member fallback remains only when no snapshot exists.

- The root project's `pnpm:devPreinstall` script now runs before resolution and linking, as it does in pnpm 11. It is skipped under `--ignore-scripts`, `--lockfile-only` and `--dry-run`, by `pnpm fetch` and `pnpm rebuild`, and by a repeat install that is already up to date. Workspaces that use the hook to prepare state the install depends on — such as [next.js](https://github.com/vercel/next.js), which generates a placeholder `next` bin with it — were left with dependents linked against files that were never created [#13313](https://github.com/pnpm/pnpm/issues/13313).

- A runtime installed through `devEngines.runtime` now matches the host when `supportedArchitectures` lists several platforms. Listing `os: [darwin, linux]` and `cpu: [x64, arm64]` used to install the runtime built for the first entry of each list, so a machine running Linux on arm64 got a macOS x64 Node.js that could not execute [#13898](https://github.com/pnpm/pnpm/issues/13898).

- The Rust CLI now honors five settings it recognized but ignored: `updateNotifier`, `legacyDirFiltering`, `initAuthorName` / `initAuthorEmail` / `initAuthorUrl`, `initLicense`, and `initVersion`. `pnpm install` and `pnpm add` check once a day for a newer pnpm and print how to get it (turn it off with `updateNotifier: false`); a `{<dir>}` filter selector can go back to matching the subtree below the directory with `legacyDirFiltering: true`; and `pnpm init` writes the configured author, license, and version into the `package.json` it scaffolds. `PNPM_CONFIG_INIT_VERSION` is now read as well.

  `maxsockets`, npm's spelling of `maxSockets`, is no longer ignored: both spellings are read from `pnpm-workspace.yaml`, the global config file, the environment, and the command line, in that increasing order of precedence — a value passed on the command line now wins even when the two sides spelled the setting differently.

  A `lastUpdateCheck` timestamp dated in the future — after a clock change, a restored snapshot, or a hand-edited state file — no longer silences the update check until that time comes around.

  `legacyDirFiltering` no longer reaches the workspace-root selectors pnpm generates for itself: the `!{<workspace-root>}` exclusion a recursive `run` / `exec` / `add` / `test` appends, and the `{<workspace-root>}` inclusion `--workspace-root` appends. Read as subtree matches they named every project below the root, so a recursive command under the setting selected nothing at all, and `--workspace-root` pulled in every project below the root instead of the root alone [#14101](https://github.com/pnpm/pnpm/issues/14101).

- The lockfile-verification line now dates a cached verdict — `✓ Lockfile passes supply-chain policies (verified 253ms ago)` — instead of the timeless `(previously verified)` [#13315](https://github.com/pnpm/pnpm/issues/13315).

- `pnpm install`, `run`, `test`, `update`, `remove`, `link`, `unlink`, `prune`, and `rebuild` now print the workspace scope they resolved — `Scope: all 41 workspace projects`, or `Scope: 5 of 41 workspace projects` under a `--filter`. This is the confirmation that a filter selected what was intended [#13315](https://github.com/pnpm/pnpm/issues/13315).

- An install that blocks a dependency's build scripts now appends a placeholder for it to `pnpm-workspace.yaml`, so approving or denying the build is an edit rather than writing the block by hand:

  ```yaml
  allowBuilds:
    es5-ext: set this to true or false
  ```

  A placeholder is not a decision — the build stays blocked until it is replaced with `true` or `false` — and an existing entry is never overwritten [#13315](https://github.com/pnpm/pnpm/issues/13315).

- Fixed `--parallel` being treated as the script name when placed before `run` in a recursive command.

- The `pnpm` wrapper's install script exits without error in the pnpm monorepo checkout, where the per-platform binary packages are not generated.

- Prevent broken-lockfile errors from including snippets of the lockfile's contents.

- When no directory above the project accepts a hard link — inside an AI agent sandbox that only grants write access to the project, or a container with just the project mounted writable — the default store is now created at `<project>/node_modules/.pnpm-store` instead of in the pnpm home directory. In those environments the home store is either read-only or on another volume, which forces every package to be copied instead of hard linked [#13525](https://github.com/pnpm/pnpm/issues/13525).

- `pnpm sbom` no longer emits components for optional platform-specific dependencies that cannot be installed on the current platform (for example, the native `@rolldown/binding-*` variants for other operating systems). Such packages are present in the lockfile but are never downloaded, so their license (and other metadata) could not be resolved and they appeared in the SBOM without one. `pnpm sbom --lockfile-only` still describes the whole lockfile graph, which is platform-independent by design.

- `pnpm sbom` now honours `--filter-prod`, the full `--filter` selector syntax (dependency queries such as `pkg...`, `{dir}` and glob paths, `[since]` change queries, exclusions), and `--workspace-root`. Selectors that match no project print `No projects matched the filters` and write no SBOM, and `--split` emits its per-project SBOMs in a stable order.

  The universal `--fail-if-no-match` flag is supported too: any filtered command whose selectors match no workspace project now exits with code 1 [#14064](https://github.com/pnpm/pnpm/issues/14064).

- `pnpm sbom` now fails with `ERR_PNPM_SBOM_MISSING_IMPORTERS` when `pnpm-lock.yaml` has no entry for a selected project, instead of writing an SBOM that under-reports that project's dependencies. Previously this crashed with `Cannot read properties of undefined (reading 'devDependencies')`.

- `scriptShell` now selects the shell for lifecycle scripts too — dependency build scripts and a project's own `preinstall`/`install`/`postinstall`/`prepare` and `pnpm:devPreinstall` — not only for `pnpm run` and `pnpm exec`. A workspace that configures a shell was still getting the platform default (`sh` / `cmd`) for everything the install itself spawns.

- `pnpm self-update` now rewrites a simple `devEngines.packageManager.version` range (`^`/`~`) to the newly installed version, keeping the operator — matching how `pnpm update` and `pnpm runtime set` rewrite ranges. Complex ranges such as `>=8.0.0` that the new version satisfies are still left unchanged [#13935](https://github.com/pnpm/pnpm/issues/13935).

- `pnpm self-update` no longer fails with `the installed pnpm wrapper is missing` when the global packages directory carries a `pnpm-workspace.yaml` of global settings (written there when a global install persists an `allowBuilds` decision). The engine install stays anchored to its own install directory instead of walking up and adopting that file as its workspace root. The `pnpm dlx` cache install gets the same anchoring, so a stray `pnpm-workspace.yaml` above the cache directory can no longer break it [#13697](https://github.com/pnpm/pnpm/issues/13697).

- `pnpm self-update <tag>` no longer downgrades when the dist-tag points at the pnpm version already running and that version is younger than `minimumReleaseAge`. The maturity cutoff moved the tag back to the previous mature release, so `pnpm self-update next-12` on v12.0.0-rc.4 switched to v12.0.0-rc.3.

- `pnpm self-update` now checks that the version it installed can run before making it the active pnpm. A release that installs but cannot execute is discarded with an error instead of replacing a working installation.

- Global commands (`pnpm add -g`, `pnpm runtime set -g`, ...) now create a missing global bin directory instead of failing with `ERR_PNPM_PNPM_DIR_NOT_WRITABLE`, and the universal `--silent` / `-s` shorthands for `--reporter=silent` (e.g. `pnpm store path --silent`) are supported again.

- `pnpm setup` no longer makes Node.js print a `MODULE_TYPELESS_PACKAGE_JSON` warning about `dist/worker.js` on every command. The `package.json` it writes next to a standalone executable now declares `"type": "module"`.

- `pnpm update` now preserves the existing range operator when updating a prerelease dependency. See pnpm/pnpm#7002.

- Kept unselected workspace link targets shallow during filtered isolated installs.

- Fixed `shamefullyHoist: true` to create public root dependency links.

- Reduced peak memory usage while resolving peer dependencies further: each occurrence in the dependency tree now shares its package id with the edge it came from instead of owning a copy of it.

- Prevented optional peers from being selected from an unrelated workspace package's shared dependency context.

- Sped up multi-importer resolution by sharing the run-resolved preferred-versions fold across importers. Every importer replayed the whole workspace's resolved-versions history into a private map each hoist round — O(importers × packages) map inserts and string clones — although the peer-hoist pickers only ever look up a handful of missing-peer names. The fold is now maintained once, workspace-wide, and importers materialize just the buckets they query. Full resolution of a 331-importer benchmark workspace dropped from 886 ms to 424 ms (peer-heavy variant: 2.8 s to 2.4 s).

- Command shims on POSIX again `exec` a target that has no interpreter — a runtime binary such as the managed Node.js, or any bin without a shebang — instead of waiting on it. A shim that waited reported a target killed by a signal as exit code `128+N` (for example `137` for `SIGKILL`), so callers that distinguish a signal death from an exit code, such as CI runners and process supervisors, saw the wrong outcome.

- pnpm now ships `node-gyp` again, so packages whose install scripts shell out to it build out of the box. Previously they failed with `spawn node-gyp ENOENT` unless a `node-gyp` was already on `PATH` — affecting `node-gyp-build` with no matching prebuild, `node-pre-gyp`, a plain `"install": "node-gyp rebuild"`, and any package shipping a `binding.gyp` without an install script. As in pnpm 11, the whole `node-gyp` dependency tree is resolved from pnpm's own lockfile when pnpm is released, so it is frozen per release rather than resolved on your machine, and `npm_config_node_gyp`, a workspace `node-gyp`, and a package's own `node-gyp` dependency all still take precedence.

- Installs are faster in workspaces that declare inter-workspace dependencies with plain ranges (`"*"`, `"^1.2.3"`) rather than the `workspace:` protocol. With `preferWorkspacePackages` enabled, linking such a dependency no longer makes a registry request that cannot change the outcome — and workspace packages that were never published no longer cost a 404 on every install.

- `.modules.yaml` now records the dependencies of a skipped optional package in `skipped` as well, matching pnpm: when a platform-incompatible optional package is skipped, its own dependency subtree is not materialized either.

- An override change is now absorbed by the fast lockfile update even when another, unchanged override uses the `catalog:` protocol. Previously any `catalog:`-valued override forced a full re-resolution whenever the override list changed, which could move unrelated packages in the lockfile (for example after `pnpm audit --fix` added an override).

- An `ssh://` git dependency pointing at a bracketed IPv6 host, such as `ssh://[::1]/repo.git`, is resolved now. Its colons were read as an SCP-style path separator, which turned the address into `[:/1]` and left the specifier unresolvable. Applies to both the TypeScript CLI and pacquet.

  In the TypeScript CLI, an `ssh://` git dependency written without user info — `ssh://git.example.com/team/repo.git`, `git+ssh://git.example.com:2222/team/repo.git` — no longer fails with `TypeError: Cannot read properties of undefined (reading 'includes')`. Only the `user@host` form worked before.

- The store index now keys URL, git-host, and `type: git` dependencies by their bare resolution id, matching the key pnpm 11 writes [#13365](https://github.com/pnpm/pnpm/issues/13365). Previously these rows carried a `<name>@` prefix, so a store warmed by one pnpm major was cold for the other and every non-registry dependency was re-downloaded, re-extracted, and re-imported on a switch. A remote tarball also occupied two index rows instead of one, doubling its extraction work.

- A stray non-directory entry in `node_modules` no longer fails an install. Files placed next to the installed dependencies are skipped rather than reported as an unreadable manifest.

- Commands in a project that pins a pnpm version no longer read the whole `pnpm-lock.yaml` to get at the leading env document. Reading stops at the end of that document, so the cost no longer grows with the rest of the lockfile: reading the env document out of an 8 MB lockfile takes ~15µs instead of ~390µs.

- Improved install performance: large tarballs are now verified and extracted while they download, so the biggest packages — whose downloads finish last — no longer add their whole extraction to the end of the install.

- Reduced peak memory usage when installing large packages. A tarball whose compressed size is at least 16 MiB, or whose registry-reported unpacked size is at least 64 MiB, is now extracted by streaming the decompression directly into the content-addressable store instead of materializing the whole decompressed archive in memory, and its large files are hashed and written to the store incrementally.

- A project that pins pnpm through `devEngines.packageManager` (or a v12+ `packageManager` field) now gets its `packageManagerDependencies` recorded in `pnpm-lock.yaml` by every command, not just by the install-family ones [#13348](https://github.com/pnpm/pnpm/issues/13348). Running `pnpm list` (or any other command) in a freshly cloned project no longer leaves the lockfile without the pinned version. The `pmOnFail` setting now also decides whether the pin is recorded: `--pm-on-fail=ignore` keeps it out of the lockfile even when the manifest asks for a stricter policy, and vice versa.

- Fixed a rare hang where `pnpm install` or `pnpm add` could wait forever: when two tasks fetched the same tarball concurrently, the waiting task could miss the downloader's completion notification and never wake up.

- Limit modern deploy lockfiles and localized virtual stores to dependencies reachable from the selected dependency groups.

- Reduced peak memory usage and allocation churn during peer dependency resolution on workspaces with many peer-dependency issue occurrences [#13681](https://github.com/pnpm/pnpm/issues/13681).

- Fixed `pnpm licenses list` to detect licenses from license files and preserve the latest package version's development classification.

- The update notification now suggests `pnpm self-update` when `PNPM_HOME` manages the pnpm in use, and the [standalone install script](https://pnpm.io/installation) otherwise — under Corepack, or when another package manager installed pnpm. `pnpm self-update` under Corepack names the standalone install script too.

- `pnpm update <name>@<version>` now fails with `ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP` when the package is not a direct dependency of any selected project, instead of quietly updating it to whatever a fresh install would resolve. There is nowhere to record the version in that case, so the request cannot be honored, and the error points at the `overrides` entry that does pin a transitive dependency. Ranges and tags are unaffected, and a package that any selected project declares directly still takes its version as before.

- Fixed proxy settings from the global `config.yaml` and command-line options in pnpm.

- `pnpm install --frozen-lockfile` no longer fails when `pnpm-lock.yaml` records the pinned pnpm version alongside an engine package the running pnpm does not install it from. An entry pinning another version is still refused, and a plain install rewrites the block [#14124](https://github.com/pnpm/pnpm/issues/14124).

- `trustPolicy: no-downgrade` no longer aborts the install with `ERR_PNPM_MISSING_TIME` on registries that serve no per-version `time` field when `minimumReleaseAgeIgnoreMissingTime` is set. The trust check reads the same publish dates the `minimumReleaseAge` check does, so it now honors the same opt-in and skips the affected package with a warning [#12446](https://github.com/pnpm/pnpm/issues/12446).

  `minimumReleaseAgeIgnoreMissingTime` no longer lets a lockfile entry the registry does not list pass the `minimumReleaseAge` check during lockfile verification. The opt-in covers a registry that cannot date its releases; a packument that does date every version it lists is saying it never published this one, which stays a hard failure.

  The missing-`time` warning now names the check it is reporting on, so a package whose `minimumReleaseAge` and `trustPolicy` checks are both skipped warns about both instead of only the first.

- `pnpm unlink` now reinstalls through the selection-aware install pipeline, matching pnpm: it honors `-r` / `--filter`, installs recursively by default inside a workspace, and supports both a shared workspace lockfile and one lockfile per project (`sharedWorkspaceLockfile: false`). Previously it always reinstalled only the active project.

- An install that drops the last dependent of a patched package no longer updates the lockfile in place and succeeds silently. Removing a dependency, widening `ignoredOptionalDependencies`, or adding a removal override could each prune the package while the patch stayed configured; such an install now falls back to a full resolution, which reports the unused patch with `ERR_PNPM_UNUSED_PATCH`. Under `allowUnusedPatches`, where the lockfile update is kept, the same install now warns that the patch went unused instead of saying nothing [#13827](https://github.com/pnpm/pnpm/issues/13827).

- `pnpm update --latest` now keeps dependencies that the npm registry does not serve in the form they were declared. A `runtime:` dependency (such as `"node": "runtime:26.5.0"`), a `git`/`github:` URL, or a remote tarball URL previously had its *name* looked up on the npm registry and its specifier overwritten with that unrelated package's version.

  `pnpm update --latest` also no longer rewrites `package.json` when a dependency is already at its latest version.

- `pnpm update --latest` now resolves a dependency declared through an `npm:` alias — directly in `package.json` or in the catalog entry a `catalog:` reference points to — to the latest version of the aliased package, keeping the `npm:<name>@` prefix in the rewritten specifier. Previously the alias name itself was looked up on the registry, failing the update with a 404 when no package of that name exists.

- `pnpm update --latest` now rewrites `jsr:` dependencies. The manifest keeps the protocol and the range operator it declared, so `jsr:1.0.0` becomes `jsr:2.0.0` and `jsr:@scope/name@^1.0.0` becomes `jsr:@scope/name@^2.0.0`, instead of being left at the old version [#13363](https://github.com/pnpm/pnpm/issues/13363).

- `pnpm update --latest` now rewrites dependencies using a named registry alias. The manifest keeps the alias prefix and the range operator it declared, so `gh:1.0.0` becomes `gh:2.0.0` and `gh:@acme/foo@^1.0.0` becomes `gh:@acme/foo@^2.0.0`, instead of being left at the old version [pnpm/pnpm#13393](https://github.com/pnpm/pnpm/issues/13393).

- Fixed `pnpm update --latest` failing with `ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_LATEST` when a dependency uses the `workspace:` (or `link:` / `file:`) protocol. Such a dependency links a local package that may not be published, so there is no registry "latest" to resolve — it is now skipped and preserved verbatim, matching the TypeScript CLI. Previously only `workspace:<path>` specifiers were skipped, so `workspace:*` / `workspace:^1.0.0` deps pointing at unpublished packages made `--latest` try to fetch them from the registry and 404.

- `pnpm update` without saving no longer records a version that the manifest's range excludes. The kept range stays authoritative: a requested version outside it is skipped with a warning, and a requested range, a dist tag, or `--latest` resolves within it instead of past it. Previously each of these could write a lockfile entry that contradicted its own specifier, which the next `pnpm install --frozen-lockfile` rejected with `ERR_PNPM_OUTDATED_LOCKFILE` [#12764](https://github.com/pnpm/pnpm/issues/12764).

- Fixed `pnpm update` rewriting exact version pins that use the `=` operator (for example `=3.5.1`) to a caret range (`^3.5.1`). Exact pins are now preserved and written back as the bare version. See pnpm/pnpm#12745.

- Added `update` and `audit` settings sections to `pnpm-workspace.yaml`, superseding the awkwardly named `updateConfig`, `auditConfig`, and top-level `auditLevel` settings:

  ```yaml
  update:
    ignoreDeps: # was updateConfig.ignoreDependencies
      - webpack
      - "@babel/*"

  audit:
    level: high # was auditLevel
    ignore: # was auditConfig.ignoreGhsas
      - GHSA-xxxx-yyyy-zzzz
  ```

  `update.ignoreDeps` lists dependency name patterns that `pnpm update` and `pnpm outdated` should skip. `audit.level` and `audit.ignore` tune `pnpm audit`.

  The deprecated `updateConfig`, `auditConfig`, and `auditLevel` settings keep working until the next major version. When both a new section value and its deprecated counterpart are set, the new section takes precedence and a warning is printed. Both the TypeScript CLI and the Rust config surface (pacquet) recognize the new sections.

- `pnpm update <pkg>@<tag>` now saves the version the dist tag resolved to in `package.json`, keeping the range operator the dependency already declared, instead of saving the tag itself. A dependency declared through a `catalog:` reference, a `workspace:` or `npm:` alias, or a path or git specifier keeps its declaration, and one that already tracks a dist tag records the tag asked for [#14092](https://github.com/pnpm/pnpm/issues/14092).

- `pnpm update <pkg>@<version>` now updates only the selected packages and leaves unrelated dependencies unchanged. A selector that renames the package it installs — `pnpm update <alias>@npm:<pkg>@<version>` or the `jsr:` equivalent — now targets the package the alias installs rather than the alias.

- `pnpm update` now writes the new version range back to `package.json` (and to the `catalog:` entry a dependency points at), instead of only updating the lockfile [#13879](https://github.com/pnpm/pnpm/issues/13879). The range operator the dependency already declared is preserved, and a dependency declared through a dist-tag (`"foo": "latest"`) keeps tracking the tag under both `pnpm update` and `pnpm update --latest`.

- **Breaking change from pnpm v11.** Under `engineStrict`, an install fails when an incompatible package is reached through a regular `dependencies` edge of an installable package, even when that whole subtree hangs off an `optionalDependencies` entry. pnpm v11 installs the package and emits an install-check warning instead. Packages reachable only through optional edges, or through a package that was itself skipped, are still skipped in both versions [#13286](https://github.com/pnpm/pnpm/issues/13286).

- A lockfile entry whose tarball resolution records no `integrity` is now reported by the lockfile-verification gate, before anything is downloaded: every offending entry is listed in one `ERR_PNPM_MISSING_TARBALL_INTEGRITY` error instead of failing the install one fetch at a time after the gate had already passed the lockfile [#13364](https://github.com/pnpm/pnpm/issues/13364). An `integrity: ''` that pins nothing is treated the same as a missing one, and the exemption for git-host archive URLs is now read from the URL rather than the lockfile's own `gitHosted` marker.

- `pnpm version <bump>` with `--dry-run` no longer edits `package.json` files. It now only reports the bumps it would make, and skips the working tree check, the version lifecycle scripts, the commit, and the tag [`pnpm/pnpm#13953`](https://github.com/pnpm/pnpm/issues/13953).

- `pnpm version -r --json` now outputs `[]` instead of human-readable text when no pending changes exist [`pnpm/pnpm#13217`](https://github.com/pnpm/pnpm/issues/13217).

- `pnpm view` now accepts the `--registry` option, matching the TypeScript CLI. Previously the flag was rejected as an unknown argument.

- When the authentication URL cannot be rendered as a QR code (for example when it exceeds the maximum QR data capacity), web-based login now displays the URL alone with a warning instead of aborting authentication [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).

- `pnpm why` and `pnpm list` no longer print stray `[90m`-style codes in their trees when the terminal supports colors. The bolded labels — the searched package in `pnpm why`, the project header and the matched package in `pnpm list` — dropped the escape byte of the styles they already carried, leaving the color codes as visible text.

- On Windows, `pnpm store path` now returns a conventional drive path without the `\\?\` verbatim prefix when the project and pnpm home are on different drives [#13987](https://github.com/pnpm/pnpm/issues/13987).

- On Windows, installation no longer fails with "A required privilege is not held by the client. (os error 1314)" when symlink creation requires elevation (e.g. Developer Mode is off) — pnpm now falls back to NTFS junctions in that case. Additionally, `pnpm clean` and `pnpm deploy --force` no longer fail with "Access is denied. (os error 5)" when removing the package links inside `node_modules` [#13694](https://github.com/pnpm/pnpm/issues/13694).

- Fixed `pnpm licenses list` to read licenses from legacy package manifest fields.

- Fixed quadratic time and memory use when resolving a large multi-project workspace from scratch. Resolving a workspace with hundreds of projects sharing thousands of packages previously took minutes and several gigabytes of memory; it now completes in seconds.

- `pnpm pack` now respects workspace-root `.npmignore` and `.gitignore` files when packing workspace packages.

- Fixed `--workspace-root` (`-w`) selecting the current workspace when `--dir` pointed at a nonexistent directory outside it (for example `pnpm --dir ../../elsewhere add -w foo`). The command now fails with `ERR_PNPM_NOT_IN_WORKSPACE`, matching pnpm. A nonexistent `--dir` inside the workspace still resolves to the workspace root as before.

- The published packages now ship a `THIRD-PARTY-NOTICES.md` file carrying the BSD 2-Clause license of the Yarn code that pnpm's hoisted-layout algorithm and built-in package-compatibility database are derived from.
