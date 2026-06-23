# pnpm

## 11.9.0

### Minor Changes

- bae694f: Some registries generate tarballs on-demand and cannot provide an integrity checksum in their package metadata. In that case pnpm now computes the integrity from the downloaded tarball and stores it in the lockfile, so the entry is verifiable on subsequent installs instead of being written without an integrity (which would fail the next install). This also applies to `--lockfile-only`: the tarball is downloaded so its integrity can be computed. A lockfile entry that is still missing its integrity is rejected as a `ERR_PNPM_MISSING_TARBALL_INTEGRITY` lockfile verification violation (the install fails closed) rather than being silently re-fetched.
- 6c35a43: Added `--exclude-peers` to `pnpm sbom`. With `auto-install-peers` (the default), peer dependencies resolve into the lockfile and are otherwise indistinguishable from the package's own dependencies. The flag drops peer dependencies (and any transitive subtree reachable only through them) from the SBOM. CycloneDX 1.7 has no scope or relationship that expresses "consumer-provided peer", so omission is the only spec-clean handling. The flag name matches `pnpm list --exclude-peers`; note the SBOM flag prunes a peer's exclusive subtree, which is stricter than `pnpm list` (which only hides leaf peers).

### Patch Changes

- 25a829e: `pnpm audit --fix` now writes a single combined `minimumReleaseAgeExclude` entry per package (e.g. `axios@0.18.1 || 0.21.1`) instead of one entry per version, matching the format documented for the setting. Existing per-version entries in `pnpm-workspace.yaml` are merged into the combined form rather than left as duplicates. Installs that auto-collect immature versions into `minimumReleaseAgeExclude` now report the same combined entries, so the "Added N entries" message matches what is written to the manifest [#12534](https://github.com/pnpm/pnpm/issues/12534).
- 1cbb5f2: Fixed non-deterministic peer resolution that could add or remove an optional transitive peer — for example `@babel/core`, reached through `styled-jsx` — from a package's peer-dependency suffix across otherwise identical installs, churning the lockfile and causing intermittent `pnpm dedupe --check` failures in CI. When a package's children are resolved by one occurrence (the "owner") and reused by a deeper consumer, whether that consumer inherited the owner's missing peers depended on whether the owner's resolution had finished yet — a race under concurrent resolution. The decision is now a function of the dependency graph's structure rather than resolution-completion order.
- d577eea: Fixed a Windows flakiness in `pnpm dlx` where a failed install could surface a spurious `EBUSY: resource busy or locked` error. The cleanup of a partially-populated dlx cache is now best-effort with retries and no longer masks the original error.
- ec7cf70: Shortened the `pnpm dlx` cache path so deep dependency trees no longer overflow Windows' `MAX_PATH`, which could make a dependency's lifecycle script fail with `spawn cmd.exe ENOENT`.
- 05b95ab: Fixed `pnpm` hanging (and crashing with an unhandled promise rejection) when a non-retryable network error such as `SELF_SIGNED_CERT_IN_CHAIN` occurs while fetching from a registry. The error is now rejected through the returned promise instead of being thrown inside the detached retry callback.
- d3f68e2: Fix a `pnpm audit` performance regression on lockfiles that contain dependency cycles. The reachable-vulnerability pruning added in pnpm 11.5.1 only memoized acyclic subtrees, so any node whose subtree touched a cycle — together with all of its ancestors — was recomputed on every query, making the path walk quadratic. Reachability is now computed once per node using Tarjan's strongly-connected-components algorithm, so cyclic graphs are handled in linear time [#12212](https://github.com/pnpm/pnpm/issues/12212).

  The audit path walk also no longer recurses, so a deeply nested dependency graph can no longer overflow the call stack, and the install path to each finding is tracked without per-node copying, keeping memory linear in the graph depth.

- 322f88f: Fix failed optional dependency updates so they don't rewrite unrelated dependency specs [#11267](https://github.com/pnpm/pnpm/issues/11267).
- 1488db1: When `enableGlobalVirtualStore` is toggled on for a project that was previously installed without it, stale hoisted symlinks under `node_modules/.pnpm/node_modules` are now replaced instead of being left pointing at the old per-project virtual store location [#9739](https://github.com/pnpm/pnpm/issues/9739).
- 6545793: Fixed `pnpm install --ignore-workspace` overwriting the `allowBuilds` map in `pnpm-workspace.yaml`. The ignored builds of a package with a build script were auto-populated into `allowBuilds` even though `--ignore-workspace` was passed, clobbering committed `true`/`false` values with the `set this to true or false` placeholder [#12469](https://github.com/pnpm/pnpm/issues/12469).
- fbdc0eb: Fixed `minimumReleaseAgeExclude` and `trustPolicyExclude` so multiple exact-version entries for the same package behave the same as a single `||` disjunction entry. Previously only the first matching rule's versions were honored, so a config like `[form-data@4.0.6, form-data@2.5.6]` could still flag `form-data@2.5.6` as violating `minimumReleaseAge`, while `[form-data@4.0.6 || 2.5.6]` worked as expected [#12463](https://github.com/pnpm/pnpm/issues/12463).
- fa7004b: The in-memory package metadata cache is now populated on the exact-version disk fast path, so repeated resolutions of the same package within one install no longer re-read and re-parse the on-disk metadata. In large monorepos this brings the time for adding a new package down from minutes to seconds. The in-memory cache key now also includes the registry, so a package of the same name served by two different registries in a single install can no longer share a cache slot and resolve the wrong tarball.
- 0a154b1: Fixed `pnpm patch` dropping the package name (and leaking internal option fields) when the patched dependency resolves to a single git-hosted version.
- 4d3fe4b: The pnpr resolver endpoints moved under the reserved `/-/pnpr` namespace: `POST /v1/resolve` is now `POST /-/pnpr/v0/resolve` and `POST /v1/verify-lockfile` is now `POST /-/pnpr/v0/verify-lockfile`. The capability handshake at `GET /-/pnpr` advertises protocol version `0` to match. This keeps every pnpr-proprietary route in npm's reserved namespace, so it can never collide with a package path.
- 0ec878d: Removing a runtime dependency now removes the matching `devEngines.runtime` or `engines.runtime` entry that was materialized from it. Blank runtime selectors are normalized to `latest`.
- 17e7f2c: `pnpm sbom` now emits a CycloneDX `issue-tracker` external reference for components (and the root) whose `package.json` declares a `bugs` URL. Email-only `bugs` entries are skipped, since the reference requires a URL.
- a84d2a1: Add `@pnpm/resolving.tarball-url`, which builds and recognizes the canonical npm tarball URL of a package. It vendors `getNpmTarballUrl` (previously the external `get-npm-tarball-url` package) and adds `isCanonicalRegistryTarballUrl`, the predicate the lockfile writer uses to decide whether a tarball URL is derivable from name+version+registry (and can therefore be omitted from `pnpm-lock.yaml`).

  Exposing `isCanonicalRegistryTarballUrl` lets a custom resolver (pnpmfile `resolvers`) fronting a proxy that serves tarballs on a non-canonical path (e.g. an ephemeral `localhost:<port>`) rewrite the resolved tarball to the canonical form, so nothing host-specific is persisted to the lockfile. Previously this logic was private to `@pnpm/lockfile.utils`.

  Two correctness fixes are included while consolidating the logic: the scoped-package unescape now handles uppercase `%2F` as well as `%2f` (percent-encoding is case-insensitive), and protocol-insensitive comparison strips only a leading `http(s)://` scheme instead of splitting on the first `://` (which could truncate URLs containing a later `://`).

- 852d537: Lockfile verification no longer reports a registry metadata fetch failure (for example a `403`/`401` on a private registry, or a network error) as `ERR_PNPM_TARBALL_URL_MISMATCH`. When the registry can't be reached to verify an entry, the install now aborts with the registry's own fetch error (such as `ERR_PNPM_FETCH_403`, which already explains the authentication situation) instead of mislabeling a transport failure as lockfile tampering. Registry fetch errors no longer leak basic-auth credentials embedded in the registry URL (`https://user:pass@host/`) into their message.

## 11.8.0

### Minor Changes

- c112b61: Added a `--dry-run` option to `pnpm install`. It runs a full dependency resolution and reports what an install would change, but writes nothing to disk (no lockfile, no `node_modules`) and always exits with code 0. This mirrors the preview semantics of `npm install --dry-run` [#7340](https://github.com/pnpm/pnpm/issues/7340).
- 179ebc4: `pnpm run --no-bail` now exits with a non-zero exit code when any of the executed scripts fail, while still running every matched script to completion. This makes the exit-code behavior of `--no-bail` consistent between recursive and non-recursive runs (recursive runs already failed at the end). Previously, a non-recursive `pnpm run --no-bail` always exited with code 0, even when a script failed [#8013](https://github.com/pnpm/pnpm/issues/8013).
- 0474a9c: Added support for generating Node.js package maps at `node_modules/.package-map.json` during isolated and hoisted installs. Added the `node-experimental-package-map` setting to inject the generated map into pnpm-managed Node.js script environments, and the `node-package-map-type` setting to choose between `standard` and `loose` package maps.
- dcededc: `pnpm sbom` now marks components reachable only through `devDependencies` with CycloneDX `scope: "excluded"` and the `cdx:npm:package:development` property. The `excluded` scope documents "component usage for test and other non-runtime purposes", which matches the semantics of a devDependency; the property is the CycloneDX npm-taxonomy marker emitted by `@cyclonedx/cyclonedx-npm`, so both modern (scope) and existing (property) consumers are covered. Components reachable at runtime (including installed `optionalDependencies`) omit `scope` and default to `required`.
- 1495cb0: Added per-package SBOM generation with `--out` and `--split` flags. Use `--out out/%s.cdx.json` to write one SBOM per workspace package to individual files, or `--split` for NDJSON output to stdout. When `--filter` selects a single package, the SBOM root component now uses that package's metadata. Workspace inter-dependencies (`workspace:` protocol) and their transitive dependencies are included. Author, repository, and license fall back to the root manifest when the package doesn't define them.
- 293921a: feat(view): support searching project manifest upward when package name is omitted

  When running `pnpm view` without a package name, the command now searches
  upward for the nearest project manifest (`package.json`, `package.yaml`, or `package.json5`) and uses its `name` field.
  If the manifest exists but lacks a `name` field, an error is thrown.

  This change also replaces the `find-up` dependency with `empathic` for
  improved performance and consistency across workspace tools.

### Patch Changes

- 29ab905: Fixed `pnpm update` overriding the version range policy of a named catalog whose name parses as a version (e.g. `catalog:express4-21`). The `catalog:` reference carries no pinning of its own, so the prefix from the catalog entry (such as `~`) is now preserved instead of being widened to `^` [#10321](https://github.com/pnpm/pnpm/issues/10321).
- bee4bf4: Security: validate config dependency names and versions from the env lockfile (`pnpm-lock.yaml`) before using them to build filesystem paths. A committed lockfile with a traversal-shaped `configDependencies` name (such as `../../PWNED`) or version (such as `../../../PWNED`) could previously cause `pnpm install` to create symlinks or write package files outside `node_modules/.pnpm-config` and the store. Names must now be valid npm package names and versions must be exact semver versions; the same validation is applied to optional subdependencies of config dependencies, and to the legacy workspace-manifest format before any lockfile is written. See [GHSA-qrv3-253h-g69c](https://github.com/pnpm/pnpm/security/advisories/GHSA-qrv3-253h-g69c).
- 96bdd57: Fix `link:` workspace protocol switching to `file:` after `pnpm rm` is run from inside a workspace package whose target workspace dependency has its own dependencies, when `injectWorkspacePackages: true` is set. Follow-up to [#10575](https://github.com/pnpm/pnpm/pull/10575), which fixed the same symptom for workspace packages without dependencies.
- 302a2f7: No longer warn about using both `packageManager` and `devEngines.packageManager` when the two fields pin the same package manager at the same version with the same integrity hash (e.g. both `pnpm@11.5.1+sha512.…`). Previously the hash was stripped from the legacy `packageManager` field but not from `devEngines.packageManager`, so even identical specifications looked like a mismatch [#12028](https://github.com/pnpm/pnpm/issues/12028).

  The warning still fires on any genuine divergence, and several cases now state the specific reason instead of a single generic message: a different package manager, a different version, or contradictory integrity hashes for the same version.

- 3f0fb21: Fixed the progress line showing leftover characters from external processes that write to the terminal between progress updates (e.g. an SSH passphrase prompt would leave a fragment like `added 0sa':`). The interactive reporter now redraws each frame in place, erasing to the end of the display before reprinting, so any such remnants are cleared [#12350](https://github.com/pnpm/pnpm/issues/12350).
- 564619f: Fixed `pnpm approve-builds` reporting "no packages awaiting approval" when a build-script dependency whose approval was revoked (e.g. after `git stash` drops the `allowBuilds` from `pnpm-workspace.yaml`) is re-added. The revoked packages are now correctly recorded in `.modules.yaml` so `approve-builds` can find them. [#12221](https://github.com/pnpm/pnpm/issues/12221)
- 3d1fd20: Skip the redundant "target bin directory already contains an exe called node" warning on Windows when the existing `node.exe` already matches the target (same hard link or identical content) [pnpm/pnpm#12203](https://github.com/pnpm/pnpm/issues/12203).
- 1b02b47: Fix macOS Gatekeeper blocking native binaries (`.node`, `.dylib`, `.so`) by removing the `com.apple.quarantine` extended attribute after importing them from the store.

  When pnpm imports files from its content-addressable store into `node_modules`, macOS preserves extended attributes, including `com.apple.quarantine`. If this xattr is present on a store blob (e.g. it was first written under a Gatekeeper-enabled app such as a Git client), it propagates to `node_modules`, and Gatekeeper blocks the native binary from loading even though pnpm already verified the file's integrity against the lockfile.

  After importing a package, pnpm now strips `com.apple.quarantine` from its native binaries, matching Homebrew's behaviour of dropping quarantine from verified downloads. The cleanup is macOS-only, runs in a single batched `xattr` call per package, is restricted to native binaries (other files are untouched), and is non-fatal (it logs a warning on unexpected errors).

  Fixes #11056

- 61969fb: Fix `pnpm install` with `optimisticRepeatInstall` incorrectly reporting `Already up to date` when `pnpm-lock.yaml` changed but project manifests did not. This affected workflows such as checking out or restoring only the lockfile [#12100](https://github.com/pnpm/pnpm/issues/12100).

  Also fixes `checkDepsStatus` to use the correct lockfile path when `useGitBranchLockfile` is enabled, so the optimistic fast-path and lockfile modification detection work with `pnpm-lock.<branch>.yaml` files instead of always stat'ing `pnpm-lock.yaml`. Merge-conflict detection now reads the resolved lockfile name as well, and with `mergeGitBranchLockfiles` enabled every `pnpm-lock.*.yaml` is scanned for modifications and conflicts. The git branch is now resolved by reading `.git/HEAD` directly (no process spawn) and uses the workspace directory rather than `process.cwd()`.

- 5c12968: Fix recursive updates of transitive dependencies when the update command mixes transitive dependency patterns with direct dependency selectors. For example, `pnpm up -r "@babel/core" uuid` now updates matching transitive `@babel/core` dependencies even when `uuid` is a direct dependency selector [#12103](https://github.com/pnpm/pnpm/issues/12103).
- 9d79ba1: Register the `pnpm update --no-save` flag in the CLI help and option parser.
- 0474a9c: Fixed `pnpm import` for Yarn v2 lockfiles when `js-yaml` v4 is installed.
- 9e0c375: Fixed `pnpm install` repeatedly prompting to remove and reinstall `node_modules` in a workspace package when `enableGlobalVirtualStore` is enabled. The post-install build step recorded a per-project `node_modules/.pnpm` virtual store directory in `node_modules/.modules.yaml`, overwriting the global `<storeDir>/links` value the install step had written. The next install then detected a virtual-store mismatch (`ERR_PNPM_UNEXPECTED_VIRTUAL_STORE`). The build step now derives the same global virtual store directory as the install step [#12307](https://github.com/pnpm/pnpm/issues/12307).
- 223d060: Document the `--cpu`, `--os` and `--libc` flags in the output of `pnpm install --help`. These flags were already supported but were only documented on the website [#12359](https://github.com/pnpm/pnpm/issues/12359).
- e85aea2: Avoid reading `README.md` from disk when publishing if the publish manifest already provides a `readme` field. The README is now only read lazily, inside `createExportableManifest`, when it is actually needed.
- 3188ae7: Fixed `pnpm peers check` to accept loose peer dependency ranges such as `>=3.16.0 || >=4.0.0-` when the installed peer version satisfies the range [#12149](https://github.com/pnpm/pnpm/issues/12149).
- 531f2a3: Fixed `pnpm update` rewriting a `workspace:` dependency that points at a local path (e.g. `workspace:../packages/foo/dist`) into a normalized `link:` or version-range specifier. Such specifiers are now preserved verbatim when the workspace protocol is preserved [#3902](https://github.com/pnpm/pnpm/issues/3902).
- fe66535: Fixed a lockfile non-convergence bug where an incremental install kept a duplicate transitive dependency that a fresh install would not produce. When a package is reused from the lockfile, its child edges are taken verbatim and bypass the preferred-versions walk, so a transitive dependency could stay pinned to an older version even after a direct dependency resolved to a higher version that satisfies the same range. The resolver now refreshes such a stale pin to the higher direct-dependency version during resolution — so the older version is never resolved or fetched, and the incremental result converges to the fresh one.
- 6d35338: `pnpm install` detects changes inside local file dependencies again. The optimistic repeat-install fast path only tracks manifest and lockfile modification times, so edits inside a local dependency's directory (or a repacked local tarball) were reported as "Already up to date". Projects with local file dependencies (`file:` and bare local path or tarball specifiers, declared directly or through `pnpm.overrides`) now always run a full install, which refetches those dependencies, matching pnpm v10 behavior [#11795](https://github.com/pnpm/pnpm/issues/11795).
- 4ca9247: Preserve the existing Node.js runtime version prefix when resolving `node@runtime:<range>` to a concrete version.
- 30c7590: Create shorter CAFS temporary package directories to leave room for lifecycle scripts that create IPC socket paths under TMPDIR.
- 13815ad: Reporter output (warnings, progress) for `pnpm store` and `pnpm config` subcommands now goes to stderr instead of stdout. This fixes scripts that capture their stdout (e.g. `PNPM_STORE=$(pnpm store path)`, `pnpm config list --json | jq`) from getting warnings mixed into the result.
- 1c05876: Avoid relinking unchanged child dependencies and remove stale child links during warm installs.
- 817f99d: Fixed lockfile churn where a package's `transitivePeerDependencies` could be dropped (and shift between packages) when the package participates in a dependency cycle. A cycle re-entry resolves against truncated children, so it must not be cached as "pure"; otherwise sibling occurrences of the same package short-circuit and lose transitive peers depending on traversal order [#5108](https://github.com/pnpm/pnpm/issues/5108).
- eba03e0: Fix `pnpm install` reporting "Already up to date" after a catalog entry in `pnpm-workspace.yaml` was reverted to a previous version. After an update modified a catalog, the workspace state cache stored the pre-update catalog versions, so reverting the entry back to its original version was not detected as an outdated state [#12418](https://github.com/pnpm/pnpm/issues/12418).
- 3b54d79: `pnpm update` now keeps lockfile `overrides` that resolve through a catalog in sync with the catalog. Previously, when an override referenced a catalog (e.g. `overrides: { foo: 'catalog:' }`) and `pnpm update` bumped that catalog entry, the lockfile's `catalogs` advanced while the resolved `overrides` kept the old version. The resulting lockfile was internally inconsistent, so a later `pnpm install --frozen-lockfile` failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH`.
- 9d0a300: Fixed `pnpm version --recursive` so it honors the workspace selection. In recursive mode the version bump now applies to the packages resolved from the workspace filter (`selectedProjectsGraph`), matching the behavior of `pnpm publish --recursive`, instead of always bumping every workspace package [#11348](https://github.com/pnpm/pnpm/issues/11348).

## 11.7.0

### Minor Changes

- Added a new setting `frozenStore` (`--frozen-store`) that lets `pnpm install` run against a package store on a read-only filesystem (e.g. a Nix store, a read-only bind mount, an OCI layer). When enabled, pnpm opens the store's SQLite `index.db` through the `immutable=1` URI — bypassing the WAL/`-shm` sidecar creation that otherwise fails on a read-only directory — and suppresses every store-write path (the `index.db` writer and the project-registry write). Pair it with `--offline --frozen-lockfile` against a fully-populated store. Under the global virtual store, package directories live inside the store, so if the store is missing the build output of a package whose lifecycle scripts are approved (or that has a patch), pnpm fails up front with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` rather than crashing mid-build on a read-only write — seed the store with those builds first. Incompatible with `--force` and with a configured pnpr server, since both write into the store; the side-effects cache is likewise not written under `frozenStore`. If the store is missing its content directory, the install fails fast with `ERR_PNPM_FROZEN_STORE_INCOMPLETE` rather than attempting to initialize it. The read-only `immutable=1` open requires Node.js >=22.15.0, >=23.11.0, or >=24.0.0; on older runtimes `--frozen-store` fails with a clear `ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE` error. Bin-linking also tolerates a read-only store: under the global virtual store a package's bin source lives inside the store, so the `chmod` that makes it executable would be refused — with `EPERM`/`EACCES`, or with `EROFS` on a genuinely read-only filesystem. That `chmod` is redundant when the seed already ships its bins executable with a normalized shebang, so it is now skipped in that case, while a non-executable bin (or one still carrying a Windows CRLF shebang) on a read-only store still errors.
- When [`pacquet`](https://github.com/pnpm/pnpm/tree/main/pacquet) (the Rust port of pnpm) is declared in `configDependencies`, pnpm now delegates dependency **resolution** to it too — not just materialization — provided the installed pacquet is new enough to support full resolving installs (>= 0.11.7).

  Previously pacquet only ran in frozen-install mode: pnpm always resolved the dependency graph itself (writing `pnpm-lock.yaml`) and handed pacquet a finished lockfile to fetch / import / link. With pacquet >= 0.11.7, a non-frozen `pnpm install` (default isolated `nodeLinker`, plain install) is delegated to pacquet end-to-end in a single pass — pacquet resolves the manifests, writes the lockfile, and materializes `node_modules`. pnpm detects the capability from the installed pacquet's version; older pacquet releases keep the resolve-then-materialize split, and `add` / `update` / `remove` still resolve in pnpm (it has to mutate the manifests first). This remains an opt-in preview of the Rust install engine [#11723](https://github.com/pnpm/pnpm/issues/11723).

- Added a new opt-in `--batch` flag to `pnpm publish --recursive` that sends all selected packages to the registry in a single `PUT /-/pnpm/v1/publish` request instead of one request per package. The target registry has to implement the batch publish endpoint (pnpr does); registries that don't are reported with a clear `ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED` error. The batch is processed all-or-nothing by pnpr: if any package in the batch fails validation, none of the packages are published.

### Patch Changes

- Reject path-traversal and reserved dependency aliases (such as `../../../escape`, `.bin`, `.pnpm`, or `node_modules`) that come from a lockfile rather than a freshly resolved manifest. A crafted lockfile alias could otherwise be joined directly under a hoisted `node_modules` directory, letting package files be written outside the intended install root or overwrite pnpm-owned layout.

  The fix adds two layers:

  - The `nodeLinker: hoisted` graph builder now validates each alias at the directory sink (`safeJoinModulesDir`), matching the validation pnpm already performs when resolving aliases from manifests.
  - The lockfile verification gate (`verifyLockfileResolutions`) now runs an always-on, policy-independent check that rejects any importer or snapshot dependency alias that is not a valid package name, failing the install early — before any fetch or filesystem work — for every node linker at once.

- Made shared package child resolution deterministic when the same package is reached through multiple contexts. pnpm now chooses the shallowest occurrence, then importer order, then parent path, instead of letting request timing decide the child context and missing-peer report [pnpm/pnpm#12358](https://github.com/pnpm/pnpm/issues/12358).
- Fix garbled summary line after submitting `pnpm update -i` and `pnpm audit --fix -i`. The interactive checkbox prompt previously printed every selected choice's full table row (label, current/target versions, workspace, URL) joined by commas, producing a wall of text after pressing Enter. The summary now lists only the selected package names (or vulnerability keys) by setting an explicit `short` per choice; the in-progress selection UI is unchanged.
- Prevent `pnpm patch-remove` from removing files outside the configured patches directory.
- Fixed `pnpm publish` ignoring `strictSsl: false` when publishing to registries with self-signed certificates. The `strictSSL` option is now forwarded to `libnpmpublish` / `npm-registry-fetch` so that `strict-ssl=false` in `.npmrc` or `strictSsl: false` in `pnpm-workspace.yaml` is respected during publish, the same way it is for `pnpm install` [pnpm/pnpm#12012](https://github.com/pnpm/pnpm/issues/12012).
- Fixed `Cannot destructure property 'manifest' of 'manifestsByPath[rootDir]' as it is undefined` regression introduced in 11.6.0 when running `pnpm add <pkg>` outside a workspace on Windows. `selectProjectByDir` was keying the resulting `ProjectsGraph` by `opts.dir` instead of `project.rootDir`, so downstream `manifestsByPath` lookups missed when the two paths normalized differently (typically drive-letter casing). [pnpm/pnpm#12379](https://github.com/pnpm/pnpm/issues/12379)
- Git dependencies that point to a subdirectory of a repository (`repo#commit&path:/sub/dir`) keep their `path` in the lockfile again. Since the integrity of git-hosted tarballs started being pinned in the lockfile, any install that actually downloaded the tarball rebuilt the lockfile resolution as `{ integrity, tarball, gitHosted }` and dropped the `path` field, while installs served from the store kept it — so the field disappeared seemingly at random. Without `path`, later installs from that lockfile silently unpacked the repository root instead of the subdirectory [#12304](https://github.com/pnpm/pnpm/issues/12304).
- Fixed nondeterministic lockfile output that made `pnpm dedupe --check` fail intermittently in CI. When a locked peer provider was pinned for a dependency that has no child dependencies of its own, the pinned provider leaked into the shared parent scope, so siblings resolved after it could pick up an optional peer they should not see. Which siblings were affected depended on resolution order, which varies with network timing.
- Sped up `pnpm install` with a frozen lockfile by running lockfile verification (the policy revalidation gate added for `minimumReleaseAge`/`trustPolicy` and the tarball-URL anti-tamper check) concurrently with fetching and linking instead of blocking the whole install on it. Dependency lifecycle scripts are still held back until verification succeeds, so no script runs on an unverified lockfile: if verification fails the install aborts before any dependency build, and if linking finishes first the install waits for the verification verdict before completing.
- User-defined `npm_config_*` environment variables are now preserved during lifecycle script execution. Previously, all `npm_`-prefixed env vars were stripped, which caused user-set variables like `npm_config_platform_arch` to be lost [pnpm/pnpm#12399](https://github.com/pnpm/pnpm/issues/12399).
- pnpm can now use different auth tokens for different package scopes, even when those scopes use the same registry URL.

  Previously, auth was selected only by registry URL. If `@org-a` and `@org-b` both used `https://npm.pkg.github.com/`, they had to share the same token. This caused problems for registries that issue tokens per organization or per scope.

  Configure a scope-specific token by adding the package scope after the registry URL in the auth key:

  ```ini
  @org-a:registry=https://npm.pkg.github.com/
  @org-b:registry=https://npm.pkg.github.com/

  //npm.pkg.github.com/:@org-a:_authToken=${ORG_A_TOKEN}
  //npm.pkg.github.com/:@org-b:_authToken=${ORG_B_TOKEN}

  //npm.pkg.github.com/:_authToken=${FALLBACK_TOKEN}
  ```

  `pnpm login --registry=https://npm.pkg.github.com --scope=@org-a` writes the token to the same scope-specific auth key.

  When installing or publishing `@org-a/*`, pnpm uses `ORG_A_TOKEN`. For `@org-b/*`, pnpm uses `ORG_B_TOKEN`. Packages without a matching scope continue to use the registry-wide fallback token.

- `pnpm setup` no longer prompts to approve build scripts for `@pnpm/exe` when installing the standalone executable. pnpm links the platform-specific binary itself, so the package's install scripts are skipped during the global self-install [#12377](https://github.com/pnpm/pnpm/issues/12377).
- Close lockfile reads deterministically before rewriting lockfiles and keep pacquet's virtual store directory length aligned with pnpm on Windows.
- A `304 Not Modified` answer from the registry now renews the cached metadata file's mtime, so the `minimumReleaseAge` freshness shortcut keeps serving resolutions from the cache. Previously, once a cached packument grew older than `minimumReleaseAge`, every subsequent install re-validated it against the registry forever, because a 304 never rewrites the file.
- Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated `@zkochan/cmd-shim` to v9.0.6.
- Fixed a Windows-only hang where a failed command could take 20–46 seconds to exit. On error, pnpm enumerates descendant processes (via `pidtree`) to terminate them, which on Windows shells out to `wmic`/PowerShell `Get-CimInstance Win32_Process` — a lookup that is extremely slow on some machines. The lookup is now bounded by a short timeout so it can no longer stall the process exit.

## 11.6.0

### Minor Changes

- `pnpm install` completes without re-resolving when `pnpm-lock.yaml` was deleted but `node_modules` is intact: the up-to-date check now treats the current lockfile (`node_modules/.pnpm/lock.yaml`) — the record of what the previous install materialized — as the wanted lockfile, verifies the manifests still match it, restores `pnpm-lock.yaml` from it, and reports "Already up to date". Previously this scenario triggered a full resolution and a re-verification of every locked package against the registry.
- 615c669: Added support for configuring URL-scoped registry settings through `npm_config_//…` and `pnpm_config_//…` environment variables, for example:

  ```text
  npm_config_//registry.npmjs.org/:_authToken=<token>
  pnpm_config_//registry.npmjs.org/:_authToken=<token>
  ```

  This provides a file-free way to supply registry authentication. Because the registry a value applies to is encoded in the (trusted) environment variable name, it is host-scoped by construction and cannot be redirected to another registry by repository-controlled config. The environment value is treated as trusted config: it takes precedence over a project/workspace `.npmrc` but is still overridden by command-line options. When the same key is provided through both prefixes, `pnpm_config_` wins.

- Raised the default network concurrency from `min(64, max(cpuCores * 3, 16))` to `min(96, max(cpuCores * 3, 64))`. Package downloads are I/O-bound, not CPU-bound, so deriving the floor from the core count left machines with few cores (for example 4-vCPU CI runners) downloading only 16 tarballs at a time and unable to saturate a low-latency registry. The `networkConcurrency` setting still overrides the default.

### Patch Changes

- Improved the warning printed when a project `.npmrc` uses an environment variable in a registry/proxy URL or in registry credentials. The message now explains why the setting was ignored and how to migrate it to a trusted source — for example by moving the line to the user-level `~/.npmrc` or running `pnpm config set "<key>" <value>` — with a link to https://pnpm.io/npmrc. The `pnpm config set` example is only suggested when the key has no `${...}` placeholder, so the snippet is always safe to copy-paste.
- Print a "Lockfile passes supply-chain policies (verified 2h ago)" message when lockfile verification is skipped because a cached verdict for the same lockfile content and policy is reused. Previously the cached short-circuit was completely silent, which made it look like the policy gate never ran [#12324](https://github.com/pnpm/pnpm/issues/12324).
- Platform-specific optional dependencies are now skipped even when their `os`/`cpu`/`libc` fields are missing from the registry metadata or the lockfile. Some registries strip these fields from the package metadata, which made pnpm download and install the binaries of every platform regardless of `supportedArchitectures`. The missing platform fields of an optional dependency are now inferred from its name (e.g. `@nx/nx-win32-arm64-msvc` → `os: win32`, `cpu: arm64`), so foreign-platform binaries are skipped without even downloading them [#11702](https://github.com/pnpm/pnpm/issues/11702).

## 11.5.3

### Patch Changes

- Stopped expanding environment variables in repository-controlled registry/proxy request destinations and registry credential values from `.npmrc`, and in workspace registry URLs from `pnpm-workspace.yaml`. Move dynamic registry URL and token configuration to trusted user, global, CLI, or environment config.
- Resolve package-manager bootstrap dependencies with trusted user or CLI registry and network config, and reject package-manager env-lockfile records that do not use registry package paths with integrity-only resolutions before auto-switch execution.
- Avoid writing `packageManagerDependencies` to `pnpm-lock.yaml` when package manager policy is set to `onFail: ignore` or `pmOnFail: ignore` [#12228](https://github.com/pnpm/pnpm/issues/12228).
- Avoid running dependency-status auto-install when the dependency status is unavailable without a project manifest.
- Using the `$` version reference syntax in `overrides` (e.g. `"react": "$react"`) now prints a deprecation warning. The syntax still works, but [catalogs](https://pnpm.io/catalogs) are the recommended way to keep an overridden version in sync with the rest of the workspace. Reference a catalog entry with the `catalog:` protocol instead.
- Fixed `pnpm config get globalconfig` to return the global `config.yaml` path again [pnpm/pnpm#11962](https://github.com/pnpm/pnpm/issues/11962).
- Fixed bare `--color` so it does not consume the following CLI flag, allowing command shorthands like `--parallel` to expand correctly and forms like `pnpm --color with current <command>` to dispatch the inner command instead of failing with `MISSING_WITH_CURRENT_CMD`.
- Fix `pnpm install` ignoring `enableGlobalVirtualStore` toggle by including it in the workspace state settings check [#12142](https://github.com/pnpm/pnpm/issues/12142).
- Security: pnpm now verifies the npm registry signature of a package-manager binary before spawning it, so a cloned repository cannot make pnpm download and execute an arbitrary native binary.

  This covers two paths that select an executable from repository-controlled input:

  - **pacquet install engine** — declaring `pacquet` (or `@pnpm/pacquet`) in `configDependencies` opts in to pnpm's Rust install engine. pnpm now verifies that the installed `pacquet` shim and the host's `@pacquet/<platform>-<arch>` binary carry a valid npm registry signature for their exact `name@version`, and refuses to run pacquet (failing the command) if the signature does not verify or cannot be checked. The only graceful fallback to pnpm's own engine is when pacquet has no binary for the current platform.
  - **automatic version switch / `self-update`** — the `packageManager` / `devEngines.packageManager` field makes pnpm download and run a specific pnpm version. pnpm now verifies the registry signature of `pnpm`, `@pnpm/exe`, and the host platform binary before installing/spawning them, and refuses to run an engine whose signature does not match a published, signed release. The check runs only on an actual download (store cache miss), so it does not add a network round trip to every command.

  In both cases the signature is verified over the _installed_ integrity, against npm's public signing keys that ship embedded in the pnpm CLI (like corepack), so bytes substituted via a tampered lockfile or a repository-controlled registry fail verification — and a registry the user did not vouch for cannot supply its own signing keys. The signed packument is fetched from the configured registry, so an npm mirror works transparently. Verification fails closed: if it cannot be completed (for example, the registry is unreachable), the command fails rather than running an unverified binary. The embedded keys are kept current by a release-time check against npm's signing-keys endpoint.

- Made peer-dependent deduplication deterministic. When a peer-suffixed package variant was a subset of two or more mutually incompatible larger variants, the variant it collapsed into depended on the order importers were resolved in, which varies between machines. This could resolve the same workspace to different lockfiles on different platforms and make `pnpm dedupe --check` alternate between passing and failing.
- Reject invalid package names and versions from staged tarball manifests before deriving filenames for `pnpm stage download`.
- Clarified in CLI help that the pnpm store is trusted shared state and store integrity checks are corruption detection, not a tamper boundary for untrusted store writers.
- Reject reserved manifest `bin` names (`""`, `"."`, `".."`, and scoped forms such as `@scope/..`) when resolving a package's bins. These names previously passed the bin-name guard and, when joined to the global bin directory during global remove/update/add operations, could resolve to the global bin directory itself or its parent and have it recursively deleted.
- Require trusted package identity before package-name `allowBuilds` entries can approve lifecycle scripts for git, git-hosted tarball, direct tarball, and local directory artifacts. To approve one of those artifacts explicitly, use its peer-suffix-free lockfile depPath as the `allowBuilds` key. Lockfile verification now rejects lockfiles where a registry-style dependency path (`name@semver`) is backed by a git, directory, or git-hosted tarball resolution (`ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`), so the dependency path is a reliable artifact identity by the time scripts can run.
- Security: pnpm now verifies the OpenPGP signature of a downloaded Node.js runtime's `SHASUMS256.txt` before trusting its integrity hashes.

  When a repository requests a Node.js runtime (e.g. via `devEngines.runtime` / `useNodeVersion`), the download mirror is repository-configurable through `node-mirror:<channel>`. The integrity of the downloaded binary was only checked against `SHASUMS256.txt` fetched from that same mirror — a circular check that a malicious mirror could satisfy by serving a tampered binary together with a matching `SHASUMS256.txt`. pnpm then executes the binary (for example to run lifecycle scripts).

  pnpm now fetches `SHASUMS256.txt.sig` and verifies the detached OpenPGP signature against the Node.js release team's public keys, which ship embedded in the pnpm CLI. A mirror that serves a tampered binary cannot also produce a valid signature, so the download fails to verify. The embedded keys are kept current by a release-time check against the canonical `nodejs/release-keys` list.

  The musl variants from the hardcoded `unofficial-builds.nodejs.org` mirror are not repository-configurable and are signed by a different key, so they continue to be trusted over TLS.

## 11.5.2

### Patch Changes

- Peer dependency resolution now reuses the peer contexts already recorded in the lockfile when those providers are still present in the dependency graph and still satisfy the peer ranges. This avoids unnecessary peer-context rewrites during lockfile regeneration. Current manifest choices remain authoritative: a newly added, explicitly updated, or aliased direct provider, a changed nested provider, or a locked version that no longer satisfies the range still takes precedence.
- The lockfile verifier now checks that a registry entry pinning an explicit `tarball` URL points at the artifact the registry's own metadata lists for that `name@version`. Previously a tampered lockfile could pair a trusted `name@version` with an attacker-chosen tarball URL (and a matching integrity for those bytes), so the install fetched the attacker's bytes. A mismatch — or any entry that can't be confirmed against the registry — is rejected with `ERR_PNPM_TARBALL_URL_MISMATCH`. Non-registry resolutions (`file:`, git-hosted, etc.) and registry entries without an explicit tarball URL (the URL is reconstructed from name+version+registry, so it is inherently bound) are unaffected; non-standard registry tarball URLs (npm Enterprise, GitHub Packages) still pass because they match the metadata.
- Fix `pnpm update --recursive --lockfile-only <pkg>@<version>` crashing with `Invalid Version` when the catalog entry for `<pkg>` is a version range (e.g. `^21.2.10`) and `catalogMode` is `strict` or `prefer`. The catalog–version comparison now skips the equality check when either side is a range rather than passing a range to `semver.eq()`, so range specifiers fall through to the existing mismatch handling instead of throwing [#11570](https://github.com/pnpm/pnpm/issues/11570).
- Avoided a Node.js crash when pnpm exits after network requests on Windows.
- Fixed packages being materialized into the virtual store without their root-level files (`package.json`, `LICENSE`, README, root entrypoints) when multiple `pnpm install` processes ran against the same store/workspace concurrently. The fast import path used to destructively empty the shared target directory, so a concurrent importer could wipe files another importer had already written; if the surviving files included the `package.json` completion marker, every later install treated the broken directory as complete and never repaired it. The fast path now imports directly only when it can create the target directory exclusively, and otherwise builds the package in a private temp directory and atomically renames it into place [#12197](https://github.com/pnpm/pnpm/issues/12197).
- Fix dependency build scripts not running under the global virtual store (`enableGlobalVirtualStore`).

  In a workspace install, dependency build scripts are deferred to a single `rebuild` pass (`buildProjects`). That pass resolved each package's location from the classic `node_modules/.pnpm/<depPathToFilename>` layout, which does not exist under the global virtual store — so native dependencies (e.g. packages using `node-gyp` / `prebuild-install`) were never built and failed to load at runtime (`Cannot find module .../build/Release/*.node`).

  `buildProjects` now resolves the global-virtual-store projection directory (`<storeDir>/links/<hash>`, computed with the same graph hash the installer uses) when `enableGlobalVirtualStore` is set, and serializes concurrent builds of the same shared projection so parallel workspace projects don't race on the same directory.

- Don't promote a `runtime:` dependency (such as the Node.js version from `devEngines.runtime` or `pnpm runtime set`) into a catalog when `catalogMode` is `strict` or `prefer`. A `runtime:` dependency round-trips to `devEngines.runtime`, which only recognizes the `runtime:` protocol; cataloging it rewrote the manifest entry to `catalog:`, which broke that round-trip, stranded it in `devDependencies`, and left `devEngines.runtime` untouched.
- Skip lockfile `minimumReleaseAge`/`trustPolicy` verification for non-registry tarball protocols (for example `file:`), so local tarball dependencies are not incorrectly checked against npm registry metadata.

## 11.5.1

### Patch Changes

- Improve `pnpm audit` performance by pruning non-vulnerable lockfile subtrees and stopping path enumeration once vulnerable findings reach the path cap.
- Avoid crashing when the workspace state cache is partially written or malformed.
- Set `npm_config_user_agent` for root lifecycle scripts during headless installs.
- Preserve the `integrity` field of a remote (non-registry) tarball dependency when its lockfile entry is rebuilt. Re-resolving such a dependency without re-fetching it (for example via `pnpm update`, or when another dependency changes) produced a resolution with no integrity — URL/tarball resolvers only learn the integrity after the tarball is downloaded — so the previously recorded integrity was dropped, making later installs fail with `ERR_PNPM_MISSING_TARBALL_INTEGRITY` [#12067](https://github.com/pnpm/pnpm/issues/12067).
- Normalize a string `repository` field into the `{ type, url }` object form when creating the publish manifest, matching npm's behavior. Some registries (e.g. Gitea/Codeberg) reject a string `repository` with a 500 Internal Server Error during `pnpm publish` [#12099](https://github.com/pnpm/pnpm/issues/12099).
- Preserve compatible optional peer versions already present in the lockfile when resolving dependencies.
- Fixed inconsistent resolution of a peer dependency that is shared through a diamond. When a package peer-depends on both another package and one of that package's own peer dependencies (for example `@typescript-eslint/eslint-plugin` peer-depends on both `@typescript-eslint/parser` and `typescript`, and `@typescript-eslint/parser` peer-depends on `typescript`), pnpm no longer reuses a hoisted instance of the shared peer that was resolved against a different version [#12079](https://github.com/pnpm/pnpm/issues/12079).

## 11.5.0

### Minor Changes

- Added a new `hoistingLimits` setting for `nodeLinker: hoisted` installs, mirroring yarn's `nmHoistingLimits`. It accepts `none` (the default — hoist as far as possible), `workspaces` (hoist only as far as each workspace package), or `dependencies` (hoist only up to each workspace package's direct dependencies). Originally proposed in [#6468](https://github.com/pnpm/pnpm/pull/6468), closing [#6457](https://github.com/pnpm/pnpm/issues/6457).
- Replaced `enquirer` with `@inquirer/prompts` for all interactive prompts. Fixes the `update -i` scrolling overflow bug where long choice lists were clipped in the terminal [#6643](https://github.com/pnpm/pnpm/issues/6643).

  **User-facing changes:**

  - `pnpm update -i` / `pnpm update -i --latest`: Scrolling now works correctly when many packages are available; the new library uses visual-line-aware pagination via `usePagination`
  - `pnpm audit --fix -i`: Same scrolling fix for vulnerability selection
  - `pnpm approve-builds`: Interactive build approval prompts updated
  - `pnpm patch`: Version selection and "apply to all" prompts updated
  - `pnpm patch-remove`: Patch removal selection updated
  - `pnpm publish`: Branch confirmation prompt updated
  - `pnpm login`: Credential prompts updated
  - `pnpm run` / `pnpm exec` (with `verifyDepsBeforeRun=prompt`): Confirmation prompt updated

  Vim-style `j`/`k` keys still work for up/down navigation in all interactive prompts.

  **Internal:** The `OtpEnquirer` and `LoginEnquirer` DI interfaces changed from `{ prompt }` to `{ input }` / `{ input, password }` respectively. Plugins or custom builds that inject their own enquirer mock will need to update.

- Staged publishes are now recognized in the trust scale. When a package version's registry metadata carries an `approver` field, it is treated as the strongest trust evidence (ranked above trusted publishers and provenance attestations), since staged publishes require 2FA publish approvals. This prevents false-positive trust downgrade errors when moving from a staged publish to a lower trust level [#11887](https://github.com/pnpm/pnpm/issues/11887).

### Patch Changes

- Fix pnpm hanging during peer resolution when an aliased install pulls in transitive packages with mutual peer cycles at different depths in the dependency tree (for example, `pnpm i nuxt@npm:nuxt-nightly@5x`). Cycles whose members hit the `findHit` cache instead of running their own `calculateDepPath` are now short-circuited by sibling resolutions at the level where the cycle is detected, so the cached path promises no longer deadlock. [#11999](https://github.com/pnpm/pnpm/issues/11999).
- Fix `pnpm dist-tag add` and `pnpm dist-tag rm` against npmjs.org failing without `--otp` with `[ERR_PNPM_UNAUTHORIZED] You must be logged in to set dist-tag … "You must provide a one-time pass. Upgrade your client to npm@latest in order to use 2FA."`. pnpm now sends `npm-auth-type: web` on dist-tag writes and surfaces the resulting OTP challenge through the existing browser-based 2FA flow (the same `withOtpHandling` helper used by `pnpm publish`), so the browser opens, the user authenticates, and the dist-tag is set on retry. `--otp=<code>` continues to work via the classic flow.
- Fix `minimumReleaseAgeExclude` handling in npm resolution fast paths so excluded packages do not get pinned to stale versions. Excludes are honored consistently during `publishedBy` metadata selection and cache-mtime shortcuts.
- Fix the `integrity` field being dropped from the lockfile entry of a remote (non-registry) https-tarball dependency when an unrelated package is installed afterwards. URL/tarball resolvers do not return an integrity (it is only known after the tarball is downloaded), so when such a dependency was reused from the lockfile without being re-fetched, its integrity was lost. It is now carried over from the existing resolution. With pnpm's lockfile-integrity hardening, the missing integrity made subsequent `--frozen-lockfile` installs fail with `ERR_PNPM_MISSING_TARBALL_INTEGRITY`. [#12001](https://github.com/pnpm/pnpm/issues/12001).
- Skip dependency re-resolution when `pnpm-lock.yaml` is missing but `node_modules/.pnpm/lock.yaml` exists and still satisfies the manifest. `pnpm install` now reuses the materialized snapshot to regenerate `pnpm-lock.yaml` instead of walking the registry to rebuild it from scratch, turning the cache+node_modules variation into a near-no-op for users who deleted the lockfile but kept the install [#11993](https://github.com/pnpm/pnpm/issues/11993).

  `--frozen-lockfile` still refuses to proceed when `pnpm-lock.yaml` is absent — the regenerated lockfile must be committed, so failing loudly is the correct behavior for CI.

## 11.4.0

### Minor Changes

- Treat tarball-integrity mismatches against the lockfile as a hard failure by default. Previously, `pnpm install` (non-frozen) would log `ERR_PNPM_TARBALL_INTEGRITY`, silently re-resolve from the registry, and overwrite the locked integrity — which meant a compromised registry, proxy, or republished version could substitute attacker-controlled content on a clean machine even though the project shipped a committed lockfile.

  `pnpm install` now exits with `ERR_PNPM_TARBALL_INTEGRITY` and a hint pointing at the new opt-in flag.

  The only opt-in is **`pnpm install --update-checksums`** — narrowly scoped to refreshing the locked integrity values from what the registry currently serves. Mirrors yarn's flag of the same name. A warning still prints when the bypass takes effect so the operation is auditable.

  `--force` and `pnpm update` deliberately do **not** bypass the integrity check. They are routine refresh operations; silently overwriting a locked integrity in those flows would erase the protection a committed lockfile is supposed to provide. `--frozen-lockfile` behavior is unchanged. `--fix-lockfile` keeps its documented purpose (filling in missing lockfile entries) and is also not a bypass.

- `pnpm runtime set <name> <version>` now saves the runtime to `devEngines.runtime` by default instead of `engines.runtime`. Pass `--save-prod` (or `-P`) to save it to `engines.runtime` instead [#11948](https://github.com/pnpm/pnpm/issues/11948).

### Patch Changes

- Fix a credential disclosure issue where an unscoped `_authToken` (or `_auth`, or `username` + `_password`, or `tokenHelper`) defined in one source — `~/.npmrc`, `~/.config/pnpm/auth.ini`, a workspace `.npmrc`, CLI flags, etc. — would be sent as an `Authorization` header to whichever registry a different (potentially untrusted) source named. The same fix extends to client TLS credentials (`cert`, `key`) so they aren't presented to a registry their author didn't choose.

  pnpm now rewrites each unscoped per-registry setting (`_authToken`, `_auth`, `username`, `_password`, `tokenHelper`, `cert`, `key`) to its URL-scoped form at load time, using the `registry=` value declared in the same source (or the npmjs default registry if the source declares none). A later layer overriding `registry=` therefore cannot pull an unscoped credential along, because it is already pinned to the URL its author intended. `ca`/`cafile` are intentionally not rescoped — they're trust anchors, not credentials, and corporate MITM-proxy setups rely on them applying globally.

  Every rescope emits a deprecation warning telling the user where the setting was pinned and how to write it directly. npm has rejected unscoped credentials outright since `npm@9`, and pnpm intends to remove support in a future major release. To target a specific registry, write the setting URL-scoped (e.g. `//registry.example.com/:_authToken=...` or `//registry.example.com/:cert=...`).

  `@pnpm/network.auth-header`: removed the `defaultRegistry` parameter from `createGetAuthHeaderByURI` and `getAuthHeadersFromCreds`. Now that credentials are URL-scoped at load time, the merged `configByUri` never contains the empty-string "default registry" placeholder slot, so re-keying it onto the merged default registry is no longer needed.

- Fix `pnpm deploy` crashing with `ENOENT: ... lstat '<deployDir>/node_modules'` when `configDependencies` declares pacquet (`pacquet` or `@pnpm/pacquet`). The deploy directory never installs config dependencies, so the install engine they designate isn't on disk to invoke; the nested install now skips them.
- Reject git resolutions whose `commit` field is not a 40-character hexadecimal SHA before invoking `git`. A malicious lockfile could otherwise smuggle a value such as `--upload-pack=<command>` through `git fetch` / `git checkout`, which on SSH or local-file transports executes the supplied command.
- Limit concurrent project manifest reads while listing large workspaces to avoid `EMFILE` errors.
- Reject patch files whose `diff --git` headers reference paths outside the patched package directory. Previously a malicious `.patch` file added via a pull request could write, delete, or rename arbitrary files reachable by the user running `pnpm install`.
- Improve the log message that pnpm prints after auto-adding entries to `minimumReleaseAgeExclude` when `minimumReleaseAge` is set without `minimumReleaseAgeStrict`. The message previously referred to the internal "loose mode" terminology, which wasn't searchable in the docs; it now tells the user to set `minimumReleaseAgeStrict` to `true` if they want these updates gated behind a prompt instead [#11747](https://github.com/pnpm/pnpm/issues/11747).
- Reject dependency aliases that contain path-traversal segments (such as `@x/../../../../../.git/hooks`) when reading them from a package manifest or symlinking them into `node_modules`. A malicious registry package could otherwise use a transitive dependency key to make `pnpm install` create symlinks at attacker-chosen paths outside the intended `node_modules` directory.
- Reject `pnpm-lock.yaml` entries whose remote tarball `resolution:` block is missing the `integrity` field. Previously the worker that extracts a downloaded tarball skipped hash verification when no integrity was supplied and minted a fresh one from the unverified bytes, so an attacker who could both alter the lockfile (e.g. via a pull request that strips `integrity:`) and serve modified content at the referenced tarball URL could install a tampered package without any error — including under `--frozen-lockfile`. pnpm now fails closed at lockfile-read time with `ERR_PNPM_MISSING_TARBALL_INTEGRITY`. Git-hosted tarballs (`gitHosted: true` or a URL on codeload.github.com / bitbucket.org / gitlab.com) and `file:` tarballs are exempt — the commit SHA in a git-host URL and the user-controlled local path already anchor the bytes.
- Validate `devEngines.runtime` and `engines.runtime` version ranges for `node`, `deno`, and `bun` when `onFail` is set to `error` or `warn`. Previously these settings only had an effect with `onFail: 'download'` — the `error` and `warn` modes silently did nothing [#11818](https://github.com/pnpm/pnpm/issues/11818). Violations now throw `ERR_PNPM_BAD_RUNTIME_VERSION`.
- Require provenance before treating trusted publisher metadata as the strongest trust evidence.

## 11.3.0

### Minor Changes

- Added `pnpm stage` with `publish`, `list`, `view`, `approve`, `reject`, and `download` subcommands for npm staged publishing.
- Added a new setting `trustLockfile`. When `true`, `pnpm install` skips the supply-chain verification pass that re-applies `minimumReleaseAge` / `trustPolicy='no-downgrade'` to every entry in the loaded lockfile. The install treats the lockfile as already-trusted — useful for closed-source projects where every commit comes from a trusted author. Defaults to `false`; verification stays on by default. Set in `pnpm-workspace.yaml`.

  Also cut the memory footprint of the verification pass itself: the per-(registry, name) trust-meta cache previously retained the full packument — dependency graphs, scripts, README, and per-version manifests — for the entire install. On large workspaces (`~4k` lockfile entries with `minimumReleaseAge` + `trustPolicy: no-downgrade` enabled) this could OOM CI runners with a 2GB heap cap. The cache now stores only the fields the trust check actually reads (`time`, per-version `_npmUser.trustedPublisher`, `dist.attestations.provenance`). The abbreviated-metadata cache is similarly projected to just the package-level `modified` field and the set of currently-listed version names. Fixes [#11860](https://github.com/pnpm/pnpm/issues/11860).

- Implemented `pnpm pkg` command natively, following `npm pkg` standards.
- Implemented `pnpm repo` command natively, following `npm repo` standards.
- Implemented `pnpm set-script` (alias `ss`) natively. Adds or updates an entry in the `scripts` field of the project manifest, supporting `package.json`, `package.json5`, and `package.yaml` formats.
- Add a `skip-manifest-obfuscation` option for `pnpm pack` and `pnpm publish`. When enabled, the original `packageManager` field and publish lifecycle scripts are kept in the packed/published manifest instead of being stripped. The pnpm-specific `pnpm` field continues to be omitted.

### Patch Changes

- Fixed `pnpm dlx` failing with `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND` when the installed package's CAS slot is missing its `package.json`. Observed in the wild for `pnpm dlx node@runtime:<version>` when the GVS slot was populated without the synthesized manifest runtime archives need (they don't ship a `package.json` of their own, so the synthesized one is the only way it gets there; an existing slot from an earlier code path that skipped the synthesis stays incomplete). The bin link itself is wired up from the resolution and remains valid, so `dlx` now falls back to the scopeless package name when the slot's manifest is unreadable — for single-bin packages (the dlx common case, including every `runtime:` spec) this matches what `manifest.bin` would have named. Multi-bin packages already require `--package=<spec> <bin>` to disambiguate and don't enter this code path.
- Fixed non-determinism in `pnpm dedupe` and `pnpm install` when a dependency graph contains packages with transitive peer dependencies on each other (e.g. `@aws-sdk/client-sts` and `@aws-sdk/client-sso-oidc`) and `auto-install-peers` is enabled. The lockfile no longer flips between two equally-valid forms across consecutive runs. The root cause was that `resolveDependencies` pushed onto its `pkgAddresses` / `postponedResolutionsQueue` arrays from inside `Promise.all`-spawned callbacks, so completion-order timing leaked into the array order and downstream cyclic-peer suffix assignment. Fixes [#8155](https://github.com/pnpm/pnpm/issues/8155).
- Fixed a regression introduced by [#11711](https://github.com/pnpm/pnpm/pull/11711) where `pnpm add <github-shorthand>` (and any other wanted-dependency whose alias can't be parsed from the user-supplied spec, e.g. tarball URLs or `pnpm/test-git-fetch#sha`) was silently dropped from the manifest update and from `pendingBuilds`. The alias-keyed lookup added in that PR couldn't find a `wantedDependency` whose `alias` was `undefined` at parse time but resolved to a package name only after fetching, so the entry never made it into `specsToUpsert`. Restored the original index-based pairing between `directDependencies` and `wantedDependencies`; the catalog-protocol preservation that PR was originally fixing is unaffected because it's driven by `rdd.catalogLookup.userSpecifiedBareSpecifier`, not by the lookup. Fixes the three `rebuilds dependencies` / `rebuilds specific dependencies` / `rebuild with pending option` failures in `building/commands/test/build/index.ts`.
- Fixed `pnpm add --config` leaving orphan entries in `pnpm-lock.env.yaml` (the optional subdependencies of the previously resolved version of the updated config dependency).

## 11.2.2

### Patch Changes

- When the install engine is delegated to pacquet via `configDependencies`, the user's CLI flags passed to `pnpm install` (e.g. `--no-runtime`, `--prod`, `--dev`, `--no-optional`, `--node-linker`, `--cpu`/`--os`/`--libc`, `--offline`, `--prefer-offline`) are now forwarded to pacquet's `install` subcommand verbatim. Previously pacquet was invoked with a fixed argument list, so flags like `--no-runtime` were silently dropped. Flag forwarding is gated on the command being `install`/`i`; `add`, `update`, and `dedupe` still don't forward (their flag surface doesn't line up with pacquet's `install`).
- Fixed `pnpm up` (and `pnpm add` / `pnpm remove`) failing with `pacquet_package_manager::outdated_lockfile` when pacquet is declared in `configDependencies`. pnpm now passes `--ignore-manifest-check` to pacquet so its `--frozen-lockfile` check doesn't fire against the (pre-mutation) `package.json` pnpm hasn't written yet [#11797](https://github.com/pnpm/pnpm/issues/11797). Requires a pacquet release that supports the flag — bump `PACQUET_VERSION` in the e2e tests once it ships.

## 11.2.1

### Patch Changes

- Mark optional subdependency snapshots of config dependencies with `optional: true` in the env lockfile, matching how optional dependencies are recorded elsewhere in `pnpm-lock.yaml`. Previously, snapshots for the platform-specific subdeps pulled in via a config dep's `optionalDependencies` were written as empty objects, which was inconsistent with the rest of the lockfile and made it look like those non-host platform variants were required.
- Fix `pickRegistryForPackage` returning the wrong registry for an unscoped `npm:` alias under a scoped local name. A manifest entry like `"@private/foo": "npm:lodash@^1"` was routing the `lodash` fetch through `registries["@private"]`, even though `lodash` is unscoped and doesn't live on that registry. The npm-alias branch now returns the alias target's own scope (or `null` for an unscoped target, falling through to `registries.default`) instead of leaking into the local key's scope.
- Don't print "Installing config dependencies..." when config dependencies are already installed and nothing needs to be fetched, re-linked, or removed.

## 11.2.0

### Minor Changes

- **Experimental:** Adding [`@pnpm/pacquet`](https://npmx.dev/package/@pnpm/pacquet) (the Rust port of pnpm) to `configDependencies` in `pnpm-workspace.yaml` now delegates the materialization phase of `pnpm install` to the pacquet binary. pnpm still owns dependency resolution; pacquet only fetches and imports from the freshly-written lockfile. This is an opt-in preview of the Rust install engine [#11723](https://github.com/pnpm/pnpm/issues/11723).

  To configure pacquet in a project, run:

  ```
  pnpm add @pnpm/pacquet --config
  ```

  You'll see changes in `pnpm-workspace.yaml` and `pnpm-lock.yaml` that should be committed. If you experience any issues with pacquet, please let us know by mentioning this in the GitHub issue you create.

- `configDependencies` now resolve and install one level of `optionalDependencies` declared by the config dependency, with `os`/`cpu`/`libc` platform filtering applied at install time. This unlocks the esbuild/swc-style pattern where a package ships platform-specific binaries via `optionalDependencies` — a config dependency can now do the same and have the matching binary symlinked next to it in the global virtual store, so `require('pkg-platform-arch')` from inside the config dependency resolves correctly.

  The env lockfile records all platform variants regardless of host platform, so it remains portable across machines. Each entry in a config dependency's `optionalDependencies` must declare an exact version — ranges and tags are rejected to keep installs reproducible.

- Implement the documented `pnpm login --scope <scope>` flag. The scope is normalized (a leading `@` is added if missing; blank values are ignored) and an `@<scope>:registry=<registry>` mapping is written to the pnpm auth file alongside the auth token. Subsequent installs of `@<scope>/*` packages then route to the chosen registry. Previously `pnpm login --scope foo` errored with `Unknown option: 'scope'` despite the flag being listed in the online documentation [#11716](https://github.com/pnpm/pnpm/issues/11716).
- `pnpm outdated` and `pnpm update --interactive` now report Node.js, Deno, and Bun runtimes installed as project dependencies (`runtime:` specifiers). Previously these were silently skipped.

### Patch Changes

- Fix `cafile=<relative-path>` in `.npmrc` being read from the wrong directory when pnpm is invoked from a different cwd (e.g. `pnpm --dir <project> install` from a CI wrapper or monorepo script). The path is now resolved against the directory of the `.npmrc` that declared it, not `process.cwd()`. Before this fix the CA file silently failed to load — the install proceeded without the configured CA and the user only saw TLS errors against a private registry, with no log line tying back to the wrongly resolved path [#11624](https://github.com/pnpm/pnpm/issues/11624).
- Fix `config.registry` getting a trailing slash appended when `registry` is set in `.npmrc` and no `registries.default` is provided by `pnpm-workspace.yaml`. The sync from `registries.default` to `config.registry` introduced in #11744 now only fires when the workspace manifest actually contributes a different default.
- Fix global add/update to handle minimumReleaseAge policy violations instead of surfacing an internal resolver guardrail error.
- Fix two crashes with `injectWorkspacePackages: true` when the lockfile has been pruned (e.g. by `turbo prune --docker`):

  - `Cannot use 'in' operator to search for 'directory' in undefined`: a peer-dependency-variant injected snapshot inherits its `resolution` from the base `packages:` entry; when a pruner drops that base entry the readers crash. `convertToLockfileObject` now reconstructs the directory resolution from the `file:` depPath at load time — a single normalization point, so every reader sees a fully-formed snapshot.
  - `ERR_PNPM_ENOENT` on `node_modules/.bin/<tool>`: after `prepare`/`postinstall`, `runLifecycleHooksConcurrently` re-imported each injected workspace package; the `scanDir`-into-`filesMap` workaround fed target-internal paths to the importer, which the `makeEmptyDir` fast path (#11088) then wiped. Drop the workaround and pass `keepModulesDir: true` so the importer preserves the target's existing `node_modules` (bin links + transitive deps) and source files keep their hardlinks.

- Fixed `pnpm login` and `pnpm logout` ignoring `registries.default` from `pnpm-workspace.yaml` [#10099](https://github.com/pnpm/pnpm/issues/10099).
- Fix the `minimumReleaseAge` (publishedBy) maturity shortcut to be inclusive at the cutoff. Previously, abbreviated metadata whose `modified` field equalled the cutoff fell off the fast path and triggered a full-metadata re-fetch (or a `MISSING_TIME` error when full metadata wasn't permitted). Since `modified` is an upper bound on every version's publish time, `modified == publishedBy` already implies every version passes the per-version `<=` filter in `filterPkgMetadataByPublishDate`, so the shortcut now accepts the boundary case directly. Strictly `>` (was `>=`) at the rejection branch.
- Honor `publishConfig.access` when publishing packages.

## 11.1.3

### Patch Changes

- `pnpm install` now re-validates `pnpm-lock.yaml` entries against the active `minimumReleaseAge` and `trustPolicy: 'no-downgrade'` policies before any tarball is fetched. Lockfiles resolved elsewhere (committed to the repo, restored from a CI cache, produced by an older pnpm) under a weaker or absent policy can no longer install a freshly-published or trust-downgraded version silently. Violating entries abort the install with `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION`, `ERR_PNPM_TRUST_DOWNGRADE`, or the generic `ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION` when both policies trip in the same batch; `minimumReleaseAgeExclude` and `trustPolicyExclude` are honored. Verification results are cached so repeat installs against an unchanged lockfile take a fast path, and pnpm shows a transient progress line while the registry round-trip runs.

  When fresh resolution picks an immature version, the behavior depends on `minimumReleaseAgeStrict`:

  - **Loose mode** — the default, in effect whenever `minimumReleaseAge` keeps its built-in 24-hour value — auto-adds the immature picks to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` and lets the install proceed. A single info message lists what was persisted.
  - **Strict mode** in an interactive terminal collects every immature direct AND transitive pick in one pass and prompts once with the full list. Approving adds them to `minimumReleaseAgeExclude` and the install continues; declining aborts before the lockfile, `package.json`, or `node_modules` is touched.
  - **Strict mode** in CI (or any non-TTY context) aborts with `ERR_PNPM_NO_MATURE_MATCHING_VERSION` listing every offending entry, instead of failing on the first one the resolver hit.

  `minimumReleaseAgeStrict` auto-enables whenever the user explicitly sets `minimumReleaseAge` (CLI flag, env var, global `config.yaml`, or `pnpm-workspace.yaml`); set `minimumReleaseAgeStrict: false` to keep loose-mode auto-collect even with an explicit `minimumReleaseAge` value. Closes [#10438](https://github.com/pnpm/pnpm/issues/10438), [#10488](https://github.com/pnpm/pnpm/issues/10488), [#11687](https://github.com/pnpm/pnpm/issues/11687).

- Allow redundant trailing base64 padding in `.npmrc` auth values and report invalid auth base64 with a pnpm error.
- Make `pnpm self-update` respect `minimumReleaseAge` (and `minimumReleaseAgeExclude`) when resolving which pnpm version to install.

  When the `latest` dist-tag points to a version newer than the configured age threshold, `self-update` now selects the newest mature version instead unless excluded by `minimumReleaseAgeExclude`.

  Also makes `dlx` and `outdated` surface invalid `minimumReleaseAgeExclude` patterns under the same `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE` error code already used by `install`, instead of leaking the internal `ERR_PNPM_INVALID_VERSION_UNION` / `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION` codes.

- Global installs respect global config build policy (e.g., `dangerouslyAllowAllBuilds` from config.yaml) when GVS is enabled [#9249](https://github.com/pnpm/pnpm/issues/9249).

  The global virtual-store (GVS) default `allowBuilds = {}` was applied before workspace manifest settings were read and before global config values (stripped by `extractAndRemoveDependencyBuildOptions`) were re-applied via `globalDepsBuildConfig`. This caused `hasDependencyBuildOptions` to return `true` (because `{}` is not null), blocking restoration of global config values like `dangerouslyAllowAllBuilds`. As a result, global installs skipped all build scripts even when the config explicitly allowed them.

  This fix moves the GVS default to **after** workspace manifest reading and `globalDepsBuildConfig` re-application, so that:

  1. Workspace manifest `allowBuilds` takes precedence (if present)
  2. Global config `dangerouslyAllowAllBuilds` is properly restored (if set and no workspace policy exists)
  3. Empty `{}` is only applied as a last resort when no policy is configured anywhere

- Honor `--silent` when `verifyDepsBeforeRun: install` auto-installs dependencies before `pnpm run` or `pnpm exec`, preventing install output from being written to stdout [#11636](https://github.com/pnpm/pnpm/issues/11636).
- Fix lockfile parsing failures when `pnpm-lock.yaml` contains CRLF line endings and multiple YAML documents [#11612](https://github.com/pnpm/pnpm/issues/11612).
- Anchor the side-effects-cache key and global-virtual-store hash to the project's script-runner Node — `engines.runtime` pin when present, shell `node` otherwise — instead of pnpm's own runtime.

  `ENGINE_NAME` (the `<platform>;<arch>;node<major>` prefix used as the side-effects-cache key and the engine portion of the GVS hash) was computed from `process.version` — the Node that runs pnpm itself. That was wrong in two situations:

  1. **`@pnpm/exe` SEA bundle.** The bundle has its own embedded Node, not the `node` on the user's `PATH` that actually spawns lifecycle scripts. Two pnpm installations on the same machine (one SEA, one npm-package) therefore disagreed on the cache key, partitioning the side-effects cache and the global virtual store across two Node majors even though both installs would run scripts on the same shell `node`.
  2. **`engines.runtime` / `devEngines.runtime` pin.** When a project pins a Node version via `devEngines.runtime` (pnpm v11+), pnpm downloads that Node into `node_modules/node/` and uses it to run lifecycle scripts. But the hash still anchored to whichever Node ran pnpm itself, not to the pinned Node — so two installs of the same project with two different runner Nodes would still disagree on the GVS slot path even though scripts run on the same pinned Node.

  Three changes:

  - `@pnpm/engine.runtime.system-node-version` now exports `engineName(nodeVersion?)`. Resolves the version in this order: explicit override → `getSystemNodeVersion()` (which already prefers `node --version` over `process.version` in SEA contexts) → `process.version`.
  - `@pnpm/deps.graph-hasher` now exports `findRuntimeNodeVersion(snapshotKeys)` — scans an iterable of lockfile snapshot keys for a `node@runtime:<version>` entry and returns its bare version string. `calcDepState` and `calcGraphNodeHash`/`iterateHashedGraphNodes` accept a `nodeVersion?` (in the options bag for the first, as a trailing parameter / ctx field for the others), forwarded to `engineName()`. The default (no override) preserves the pre-change behaviour. The legacy `ENGINE_NAME` constant in `@pnpm/constants` is unchanged so external consumers and existing tests keep working; in non-SEA, non-pinned contexts every value lines up.
  - Every install-side caller of the graph-hasher (`@pnpm/installing.deps-resolver`, `@pnpm/installing.deps-restorer`, `@pnpm/installing.deps-installer`, `@pnpm/building.during-install`, `@pnpm/building.after-install`, `@pnpm/deps.graph-builder`) now derives the project's pinned runtime via `findRuntimeNodeVersion(Object.keys(graph))` once per invocation and threads it through.

  On upgrade, two one-time GVS slot churns are possible:

  - **SEA-pnpm users** without a runtime pin: slots that previously hashed under the embedded-Node major (e.g. `node26`) now hash under the shell-Node major (e.g. `node24`), matching what pacquet, the npm-published `pnpm` package, and any other pnpm-compatible tool already produce.
  - **Projects with a `devEngines.runtime` pin**: slots that previously hashed under the runner's Node major now hash under the pinned Node major, matching what the lifecycle scripts will actually run on.

  In both cases the old slots become prune-eligible.

- Resolve the GVS hash's engine portion per-snapshot when a dependency declares its own `engines.runtime`, instead of using an install-wide value.

  Pnpm's resolver desugars a dep's `engines.runtime` into `dependencies.node: 'runtime:<version>'`, and the bin linker spawns that dep's lifecycle scripts through the pinned Node downloaded into `<pkgDir>/node_modules/node/`. The GVS hash and the side-effects-cache key prefix were still anchored to the install-wide runtime — so a pinning snapshot's slot encoded the wrong Node major, and a reinstall on the same host could read the cached side-effects under a key whose `<platform>;<arch>;node<major>` triple disagreed with the Node the build actually ran on.

  Per-snapshot resolution now matches what `bins/linker` already does on a per-package basis:

  - `@pnpm/deps.graph-hasher` adds `readSnapshotRuntimePin(children)` — reads the `node` entry from one snapshot's graph children and extracts the version from a `node@runtime:` value. Pairs with the existing `findRuntimeNodeVersion(snapshotKeys)` install-wide fallback (also now exported from `@pnpm/deps.graph-hasher` rather than `@pnpm/engine.runtime.system-node-version`, where it was a poor fit — `system-node-version` is about probing the host Node, not parsing lockfile-derived strings).
  - `calcDepState` and `calcGraphNodeHash` consult `readSnapshotRuntimePin(graph[depPath].children)` first and only fall back to the install-wide `nodeVersion` parameter when the snapshot doesn't pin its own Node.

  Pacquet mirrors the same precedence at the `calc_graph_node_hash` call site in `package-manager/src/virtual_store_layout.rs` — a new `find_own_runtime_node_major(snapshot)` helper reads each snapshot's `dependencies` for a `node` entry with `Prefix::Runtime` and overrides the install-wide engine when present.

  On upgrade, snapshots of dependencies that declare their own `engines.runtime` re-hash under that dep's pinned Node instead of the install-wide value. The old slots become prune-eligible. Closes [#11690](https://github.com/pnpm/pnpm/issues/11690).

- Fixed `pnpm publish` failing with a 404 when authentication relied on OIDC trusted publishing alongside an `.npmrc` written by `actions/setup-node` (`_authToken=${NODE_AUTH_TOKEN}`) without `NODE_AUTH_TOKEN` being set. Unresolved `${VAR}` placeholders in auth values are now treated as empty rather than passed through verbatim, so the literal placeholder no longer surfaces as a bearer token when OIDC fallback is the intended auth source [#11513](https://github.com/pnpm/pnpm/issues/11513).
- Fix `devEngines.packageManager` (singular form, without `onFail`) defaulting to `onFail: "error"` instead of the documented `pmOnFail: "download"`. As a result, a project that pinned a different pnpm version via `devEngines.packageManager` and ran `pnpm install` from a mismatched pnpm version failed with a hard error, even though the migration table from `managePackageManagerVersions: true` to `pmOnFail: download (default)` promises the install would auto-download the wanted version [#11676](https://github.com/pnpm/pnpm/issues/11676).

  The array form of `devEngines.packageManager` keeps its existing per-element defaults (`error` for the last entry, `ignore` for the rest), since those reflect explicit prioritization by the user. Explicit `onFail` values continue to win.

- Fix `devEngines.packageManager` not writing `packageManagerDependencies` to `pnpm-lock.yaml` when the lockfile lacks an env-doc entry. Previously the lockfile sync skipped resolution unless an existing `packageManagerDependencies.pnpm` entry needed refreshing, so a fresh install without `onFail: "download"` left the resolved pnpm version unrecorded — contradicting the documented behavior that the resolved version is stored in `pnpm-lock.yaml` [#11674](https://github.com/pnpm/pnpm/issues/11674).
- Warn when `package.json` contains a legacy `pnpm` field with settings pnpm no longer reads from `package.json` (e.g. `pnpm.overrides`, `pnpm.patchedDependencies`). Previously these were silently ignored after the upgrade from v10, leaving users unaware that their overrides/patched dependencies had stopped taking effect [#11677](https://github.com/pnpm/pnpm/issues/11677).

## 11.1.2

### Patch Changes

- `convertEnginesRuntimeToDependencies`: switch the runtime-dependency write to `Object.defineProperty` so the CodeQL `js/prototype-polluting-assignment` rule treats the assignment as safe regardless of the property name (follow-up to [#11609](https://github.com/pnpm/pnpm/pull/11609)).
- Address CodeQL static-analysis findings: guard manifest dependency writes against prototype-polluting keys (`__proto__`, `constructor`, `prototype`), and replace a potentially super-linear semver-detection regex in registry 404 hints with an O(n) parser.
- Strip `sec-fetch-*` headers from outgoing HTTP requests. These headers are automatically added by undici's `fetch()` implementation per the Fetch spec but cause Azure DevOps Artifacts to return HTTP 400 for uncached upstream packages, as ADO interprets them as browser requests [#11572](https://github.com/pnpm/pnpm/issues/11572).
- Fix `minimumReleaseAge` handling for cached abbreviated metadata.

  The version-spec cache fast path no longer rethrows `ERR_PNPM_MISSING_TIME` under `strictPublishedByCheck`; it now falls through to the registry-fetch path, consistent with the adjacent mtime-gated cache block.

  When the registry returns 304 Not Modified for a package whose cached metadata is abbreviated (no per-version `time`), pnpm now re-fetches with `fullMetadata: true` if `minimumReleaseAge` is active and the package was modified after the cutoff. The upgraded metadata is persisted to disk so subsequent installs don't repeat the fetch. Previously the abbreviated meta was used as-is and the maturity check fell back to its warn-and-skip path, silently bypassing the quarantine and emitting a misleading "metadata is missing the time field" warning.

  Closes #11619.

- Fix `pnpm upgrade --interactive --latest -r` not respecting named catalog groups. Previously, upgrading a dependency using a named catalog (e.g. `"catalog:foo"`) would incorrectly rewrite `package.json` to `"catalog:"` and place the updated version in the default catalog instead of the named one [#10115](https://github.com/pnpm/pnpm/issues/10115).
- Fixed `optimisticRepeatInstall` skipping `pnpm-lock.yaml` merge conflict resolution when the existing `node_modules` state appears up to date.
- Fix `minimumReleaseAge` / `resolutionMode: time-based` installs failing on lockfiles whose `time:` block is missing entries. The npm-resolver's peek-from-store fast path now surfaces `publishedAt` from the lockfile rather than discarding it, and falls through to a registry metadata fetch when the time-based cutoff can't be computed from the data on hand.

## 11.1.1

### Patch Changes

- Skip installability validation when scanning workspace projects in `checkDepsStatus` (run by `verifyDepsBeforeRun`). Previously the status check called `findWorkspaceProjects`, which validates each project's `engines` and `os`/`cpu`/`libc` and warns about useless fields in non-root manifests — work that the install pipeline already performs. With no `nodeVersion` threaded through, the engine check also fell back to the system Node from `PATH` and emitted spurious "Unsupported engine" warnings before scripts ran. Status-only callers now use `findWorkspaceProjectsNoCheck`; install paths continue to validate.
- Fixed `pnpm add <alias>:@scope/pkg` for [named registries](https://github.com/pnpm/pnpm/pull/11324). The local resolver was claiming any specifier containing `/` as a local directory, so `pnpm add bit:@teambit/bit` (with `bit` configured under `namedRegistries`) installed a bogus link to `bit:@teambit/bit/` instead of resolving from the configured registry. The local resolver now runs after the named-registry resolver in the resolution chain.
- Updated `@zkochan/cmd-shim` to 9.0.3. The sh shim it writes for `.cmd` / `.bat` targets now escapes the `/C` switch as `//C`, so it survives the path translation Git Bash applies when launching `cmd.exe`. Without this, a bare `/C` was rewritten to `C:\` before reaching cmd.exe — the switch was dropped, cmd started interactively, and the calling script saw the cmd banner instead of the wrapped command's output. Affects any cmd-shim-wrapped batch script invoked from Git Bash / MSYS / Cygwin on Windows. See [pnpm/cmd-shim#55](https://github.com/pnpm/cmd-shim/pull/55).

## 11.1.0

### Minor Changes

- Added `pnpm audit signatures` to verify ECDSA registry signatures for installed packages against keys from `/-/npm/v1/keys` [#7909](https://github.com/pnpm/pnpm/issues/7909). Scoped registries are respected, and registries without signing keys are skipped.
- Added support for installing packages from the [GitHub Packages npm registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry) via a built-in `gh:` prefix (e.g. `pnpm add gh:@acme/private`), and, more broadly, for arbitrary named registries in the style of [vlt's named-registry aliases](https://docs.vlt.sh/cli/registries). Authentication is picked up from the existing per-URL `.npmrc` entries (e.g. `//npm.pkg.github.com/:_authToken=...`), so no separate auth mechanism is required.

  Additional aliases — or an override for the built-in `gh` alias, for GitHub Enterprise Server — can be configured under `namedRegistries` in `pnpm-workspace.yaml`:

  ```yaml
  namedRegistries:
    gh: https://npm.pkg.github.example.com/
    work: https://npm.work.example.com/
  ```

  With this, `work:@corp/lib@^2.0.0` resolves against `https://npm.work.example.com/`. [#11324](https://github.com/pnpm/pnpm/issues/11324).

- Allow setting sbom spec version using `--sbom-spec-version` [#11389](https://github.com/pnpm/pnpm/pull/11389).
- Add `--no-runtime` flag (config: `runtime=false`) to skip installing runtime entries (e.g. Node.js downloaded via `devEngines.runtime`) without modifying the lockfile. The lockfile keeps the runtime entry so frozen-lockfile validation still passes; only the runtime fetch and `.bin` linking are skipped. Useful in CI matrices where the runtime is provisioned externally (e.g. via `pnpm runtime -g set node <version>`) before `pnpm install` runs.
- Added the `pnpm bugs` command that opens a package's bug tracker URL in the browser. With no arguments, it reads the current project's `package.json`; with one or more package names, it fetches each package's metadata from the registry and opens its bug tracker. Falls back to `<repository>/issues` when the `bugs` field is missing [#11279](https://github.com/pnpm/pnpm/pull/11279).
- Added `pnpm owner` command to manage package owners on the registry.

### Patch Changes

- Added "published X ago by Y" information to the `pnpm view` command output, similar to `npm view`. This is useful when comparing against `minimumReleaseAge`.

  For example, `pnpm view pnpm` now shows:

  ```
  published 17 hours ago by GitHub Actions
  ```

- `pnpm publish` now honors the configured HTTP/HTTPS proxy (including `https_proxy`/`http_proxy`/`no_proxy` environment variables) when polling the registry's `doneUrl` during the web-based authentication flow. Previously the poll bypassed the proxy, causing the registry to respond `403` from a different source IP and the login to never complete [#11561](https://github.com/pnpm/pnpm/issues/11561).
- `pnpm add -g` now installs each space-separated package into its own isolated directory by default. To bundle multiple packages into the same isolated install (so that they share dependencies and are removed together), pass them as a comma-separated list. For example:

  - `pnpm add -g foo bar` installs `foo` and `bar` as two independent globals — removing one does not affect the other.
  - `pnpm add -g foo,bar qar` bundles `foo` and `bar` into a single isolated install while `qar` is installed on its own.

  Related: [#11587](https://github.com/pnpm/pnpm/issues/11587).

- `pnpm runtime set <name> <version>` no longer fails in the root of a multi-package workspace with the `ADDING_TO_ROOT` error. Installing the workspace root is a valid target for a runtime, so the command now bypasses that safety check.
- Fix `pnpm --version` hanging for the lifetime of the worker pool after the version was printed. `main.ts`'s `--version` short-circuit returned before reaching the command-handler `finally` that calls `finishWorkers()`, so the worker pool that `switchCliVersion` had spawned during integrity resolution stayed alive and held the Node event loop open. The CLI entry now runs `finishWorkers()` from its own `finally`, so every exit path tears the pool down.

  Repro: `pnpm --version` in a workspace whose `devEngines.packageManager` version already matches the running pnpm + `onFail: "download"`. `switchCliVersion` resolves the integrity (spawning workers), finds nothing to swap, returns. The version prints, then the process hangs.

## 11.0.9

### Patch Changes

- Fixed installation of GitLab-hosted dependencies. pnpm now downloads the tarball from `https://gitlab.com/<user>/<project>/-/archive/<sha>/<project>-<sha>.tar.gz` instead of the GitLab API endpoint that contained an encoded slash (`%2F`) between user and project. The encoded slash both triggered `406 Not Acceptable` responses from GitLab and produced virtual store directory names that Node refused to import (`ERR_INVALID_MODULE_SPECIFIER`) [#11533](https://github.com/pnpm/pnpm/issues/11533).
- Honor `NPM_CONFIG_USERCONFIG` (and its lowercase `npm_config_userconfig` form) as a low-priority fallback when locating the user-level `.npmrc`. This restores compatibility with environments that point npm at a custom auth file via that env var — most notably `actions/setup-node`, which writes registry credentials to `${runner.temp}/.npmrc` and exports `NPM_CONFIG_USERCONFIG` to reference it. Without this, GitHub Actions workflows using `actions/setup-node` to authenticate to private registries broke after upgrading to pnpm v11. PNPM-prefixed env vars and `npmrcAuthFile` from the global `config.yaml` continue to take precedence [#11539](https://github.com/pnpm/pnpm/issues/11539).
- Fix `pnpm pack` not bundling dependencies listed in `bundleDependencies` (or `bundledDependencies`). The npm-packlist upgrade in pnpm 11 changed its API to require the caller to pre-populate the dependency tree, which the wrapper was not doing — `bundleDependencies` were silently dropped from the tarball [#11519](https://github.com/pnpm/pnpm/issues/11519).
- Fixed the pnpm CLI crashing with a confusing `SyntaxError: Invalid regular expression flags` instead of printing a clear "requires Node.js v22.13" error when launched on an unsupported Node.js version. The Node.js version check in `bin/pnpm.mjs` was effectively dead code because the static `import` of the bundled `dist/pnpm.mjs` was hoisted by the ES module loader and parsed before the check could run [#11546](https://github.com/pnpm/pnpm/issues/11546).
- Fixed `pnpm --prefix=<dir> install` overwriting the existing `pnpm-workspace.yaml` in `<dir>` with `set this to true or false` placeholders. The renamed `--prefix` option (which maps to `dir`) was not honored when locating the workspace root, so the workspace manifest's `allowBuilds` settings were not loaded into config and got clobbered when ignored builds were auto-populated [#11535](https://github.com/pnpm/pnpm/issues/11535).
- Fixed `pnpm publish --provenance` failing with a 422 from the registry when the package version contained semver build metadata (e.g. `1.0.0-canary.0+abc1234`). The `+<build>` segment is now stripped before packing so that the version embedded in the tarball, the metadata sent to the registry, and the sigstore provenance subject all agree [#11518](https://github.com/pnpm/pnpm/issues/11518).

## 11.0.8

### Patch Changes

- Restored the heuristic that preserves tarball URLs in `pnpm-lock.yaml` when they cannot be derived from name+version+registry, even with the default `lockfileIncludeTarballUrl: false`. Without this, `pnpm install --frozen-lockfile` from an empty store fails with `ERR_PNPM_FETCH_404` for packages on registries that serve tarballs from a non-standard path — most notably GitHub Packages (`https://npm.pkg.github.com/download/<scope>/<name>/<version>/<hash>`) and JSR. `lockfileIncludeTarballUrl: true` continues to force the URL into the lockfile for every package [#11276](https://github.com/pnpm/pnpm/issues/11276).
- Run `preversion`, `version`, and `postversion` lifecycle scripts for `pnpm version`.
- Fixed `ERR_PNPM_BAD_TARBALL_SIZE` when a registry serves tarballs with an end-to-end `Content-Encoding` (e.g. `gzip`). Tarballs are already compressed, so the fetcher now requests them with `Accept-Encoding: identity` (matching pnpm v10's effective behavior) and, as defense in depth against misbehaving servers, no longer enforces the strict `Content-Length` check when the response declares a `Content-Encoding` — `Content-Length` in that case refers to the encoded payload, not the decoded bytes the fetch implementation yields [#11506](https://github.com/pnpm/pnpm/issues/11506).

## 11.0.7

### Patch Changes

- Restore the execute bit on the `node-gyp` shims packed inside `@pnpm/exe` (`dist/node-gyp-bin/node-gyp`, `dist/node-gyp-bin/node-gyp.cmd`, and `dist/node_modules/node-gyp/bin/node-gyp.js`). Without this, `pnpm/action-setup`'s standalone path (used on runners with Node.js < 22.13) failed any install whose lifecycle script invoked `node-gyp rebuild` with `sh: 1: node-gyp: Permission denied` [#11483](https://github.com/pnpm/pnpm/issues/11483).
- Fixed the `pn`, `pnpx`, and `pnx` aliases failing in Git Bash / MSYS2 on Windows when pnpm was installed via `@pnpm/exe` (or after `pnpm self-update`) [#11486](https://github.com/pnpm/pnpm/issues/11486). Running `pnpx` (or `pnx`) printed the cmd.exe banner and dropped the user into an interactive command prompt instead of running `pnpm dlx`. The `bin` field rewrite on Windows was pointing those aliases at `.cmd` files; cmd-shim's Bash shim for a `.cmd` target wraps it in `exec cmd /C ...`, and MSYS2 mangles `/C` into a Windows path before cmd.exe sees it. The aliases are now `.exe` hardlinks of the SEA binary, which detects which name it was launched as via `process.execPath` and prepends `dlx` for `pnpx` / `pnx`.
- Fix `pnpm install` recreating `node_modules` after `pnpm fetch`. `pnpm fetch` records empty `hoistPattern` and `publicHoistPattern` in `.modules.yaml`; since v11 removed the explicit-config gate, the follow-up install treated those as a hoist-pattern change and purged the modules directory. The fetch step now flags the modules manifest with `virtualStoreOnly: true` so the next install skips the hoist-pattern comparison and completes the missing post-import linking in place [#11488](https://github.com/pnpm/pnpm/issues/11488).
- Pin the integrity of git-hosted tarballs (codeload.github.com, gitlab.com, bitbucket.org) in the lockfile so that subsequent installs detect a tampered or substituted tarball and refuse to install it. Previously the lockfile only stored the tarball URL for git dependencies, so a compromised git host or a man-in-the-middle could serve arbitrary code on later installs without lockfile changes.

  A new `gitHosted: true` field is recorded on git-hosted tarball resolutions in the lockfile, letting every reader/writer route them by a single typed check instead of pattern-matching the tarball URL in each call site. Lockfiles written by older pnpm versions are enriched on load (URL fallback) so the field can be relied on uniformly across the codebase.

- Allow user-level preferences in the global `config.yaml`. The following settings can now be set in `~/.config/pnpm/config.yaml` (or via `pnpm config set --location global`) instead of being restricted to `pnpm-workspace.yaml`: `agent`, `globalVirtualStoreDir`, `initPackageManager`, `initType`, `registrySupportsTimeField`, `scriptShell`, `shellEmulator`, `sideEffectsCache`, `sideEffectsCacheReadonly`, `stateDir`, `strictDepBuilds`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, `updateNotifier`, `useStderr`, `verifyDepsBeforeRun`, `verifyStoreIntegrity`, `virtualStoreDir`, `virtualStoreDirMaxLength` [#11474](https://github.com/pnpm/pnpm/issues/11474).
- Make trusted publishing (OIDC) take precedence over a configured static `_authToken` in `pnpm publish`, mirroring the npm CLI's behavior. When OIDC succeeds, the OIDC-derived token overrides any pre-configured `_authToken`; when OIDC is not applicable (no CI environment, exchange fails, registry has no trusted publisher configured), the static token is used as a fallback. This applies on every package during recursive publish, so each workspace package independently attempts trusted publishing.

  Additionally, the `NPM_ID_TOKEN` env var is now honored as a CI-agnostic injection point for an OIDC ID token. Previously OIDC was only attempted on GitHub Actions or GitLab; now any CI provider that exposes its own OIDC mechanism (e.g. CircleCI's `CIRCLE_OIDC_TOKEN_V2`, Buildkite, etc.) can forward its token via `NPM_ID_TOKEN` and trusted publishing will work without pnpm needing to recognize the provider explicitly.

- `--pm-on-fail=ignore` (and other universal options like `--loglevel`, `--reporter`) is now honored when combined with `--help` or `--version`. Previously the CLI argument parser short-circuited those flags before universal options were preserved, so `pnpm audit --pm-on-fail=ignore --help` and `pnpm --pm-on-fail=ignore --version` reported the strict packageManager mismatch instead of running the requested action [#11487](https://github.com/pnpm/pnpm/issues/11487).
- Fix a regression where `pnpm --recursive --filter '!<pkg>' run/exec/test/add` would include the workspace root in the matched projects. The workspace root is now correctly excluded by default when only negative `--filter` arguments are provided, matching the [documented behavior](https://pnpm.io/cli/recursive). To include the root, pass `--include-workspace-root` [#11341](https://github.com/pnpm/pnpm/issues/11341).
- Restore npm-CLI-compatible `--json` stdout output for `pnpm publish` ([#11476](https://github.com/pnpm/pnpm/issues/11476)). pnpm 11 reimplemented publish natively ([#10591](https://github.com/pnpm/pnpm/pull/10591)) and inadvertently dropped the per-package JSON object that pnpm 10 emitted transitively via the npm CLI, silently breaking downstream tooling — most notably `nx release publish`, which parses stdout JSON to confirm success ([nrwl/nx#35575](https://github.com/nrwl/nx/issues/35575)). On success, the output is now:

  - `pnpm publish --json` → single object `{ id, name, version, size, unpackedSize, shasum, integrity, filename, files, entryCount, bundled }`, mirroring `npm publish --json`.
  - `pnpm publish -r --json` → array of those objects, mirroring `pnpm pack --json`'s shape choice.
  - `pnpm publish -r --report-summary` → existing `pnpm-publish-summary.json` envelope `{ publishedPackages: [...] }` is preserved, but each entry is upgraded to the same per-package shape (additive — `name` and `version` are still present).

- `pnpm config get @<scope>:registry` now reports the same URL that `pnpm publish` and the resolvers actually use. Previously, `config get` only consulted `.npmrc`, while `publish`/install used the merged map that includes `pnpm-workspace.yaml`'s `registries` block — so the two could diverge silently and a publish could go to the wrong registry [#11492](https://github.com/pnpm/pnpm/issues/11492).

## 11.0.6

### Patch Changes

- Fix `pnpm_config_npmrc_auth_file` and `pnpm_config_userconfig` env vars not actually loading the custom `.npmrc`. The env vars were parsed and assigned to the resolved config, but only after `loadNpmrcConfig` had already read the default `~/.npmrc` — so the custom file path was set but never read. The relevant env vars are now consulted before the user-level `.npmrc` is loaded [#11465](https://github.com/pnpm/pnpm/issues/11465).
- Preserve the original key order in `pnpm-workspace.yaml` when updating it. Existing keys keep their position, and new keys are inserted in alphabetical position when the existing keys are already sorted (with a leading `packages` key allowed) or appended at the end otherwise.
- Fixed `pnpm self-update` on installations originally set up by pnpm v10. v10 added `PNPM_HOME` directly to PATH and wrote a `pnpm` bootstrap shim there. v11 setup writes shims under `PNPM_HOME/bin` instead, so when a v10 user upgrades to v11 the legacy shim at `PNPM_HOME` keeps pointing into the old `.tools/<version>` install — `pnpm --version` continues to report the pre-update version even though the new version was installed under `global/v11`. Self-update now detects this layout, refreshes the legacy shims so the upgrade actually takes effect, and prints a hint suggesting `pnpm setup` to migrate PATH to the v11 layout. [#11464](https://github.com/pnpm/pnpm/issues/11464).
- Print a warning when settings that are not allowed in the global config file (e.g. `nodeLinker`, `hoistPattern`) are present in `config.yaml` and silently ignored. Previously these settings were dropped without any feedback, leaving users unsure why their global configuration had no effect. The warning suggests moving those settings to a project-level `pnpm-workspace.yaml`, or sharing them across projects via [config dependencies](https://pnpm.io/11.x/config-dependencies).
- Throw a pnpm error when `overrides` has an invalid shape or contains a non-string value.
- Validate all `readPackage` dependency map fields, including `devDependencies`, and reject falsy non-object invalid values instead of silently accepting them.
- Prevent crashes during `pnpm config`, `pnpm set`, and `pnpm get` by tolerating `configDependencies` install failures. For these commands, a failure to install `configDependencies` (for example because the registry auth token has not been written yet) is now logged at debug level and the command proceeds. All other commands still surface the install error [#10684](https://github.com/pnpm/pnpm/issues/10684).
- Treat `allowBuilds` as an install-state input and clear previously ignored builds when they are explicitly disallowed.
- Fixes #10594, catalogs not being read from the workspace when using the `catalog:` protocol with the `pnpm dlx` / `pnpx` command, resulting in a catalog entry not found error.
- Accept `PNPM_CONFIG_*` (uppercase) environment variables in addition to `pnpm_config_*`. Previously, only the lowercase form was honored, so env vars renamed per the v11 migration guide (e.g. `PNPM_CONFIG_USERCONFIG`) silently had no effect on case-sensitive systems like macOS and Linux [#11465](https://github.com/pnpm/pnpm/issues/11465).

## 11.0.5

### Patch Changes

- Drop the `darwin-x64` artifact from `@pnpm/exe` and from the GitHub release page. The Node.js SEA mechanism `pnpm pack-app` uses produces a binary that segfaults at startup on Intel Macs because of an upstream Node.js bug ([nodejs/node#62893](https://github.com/nodejs/node/issues/62893), tracked alongside [#59553](https://github.com/nodejs/node/issues/59553); the Node.js team has [opted not to fix it](https://github.com/nodejs/node/pull/60250) on the grounds that x64 macOS is being phased out). Re-signing with `codesign` or `ldid` doesn't help — the corruption is in LIEF's Mach-O surgery, before signing.

  Intel Mac users should install pnpm via `npm install -g pnpm` (uses the system Node.js, no SEA), or stay on pnpm 10.x. `@pnpm/exe`'s preinstall on Intel Mac now exits with a clear error pointing at these alternatives.

  Closes [#11423](https://github.com/pnpm/pnpm/issues/11423).

- `pnpm dlx` (and `pnpx`/`pnx`/`pnpm create`) now runs the same interactive `approve-builds` prompt as `pnpm add -g` when the package being launched depends on transitive packages with install scripts. Previously, the v11 `strictDepBuilds` default made dlx fail with `ERR_PNPM_IGNORED_BUILDS` and required users to re-run with `--allow-build=<pkg>` for every offending dependency. dlx also now removes the partially-populated cache directory when the install fails, so a subsequent run starts clean instead of reusing a broken install whose builds were silently skipped [#11444](https://github.com/pnpm/pnpm/issues/11444).
- 72629fc: Fix `pnpm -g ls --json` and `pnpm -g ls --parseable` so they emit valid JSON and parseable output respectively, matching pnpm 10 behavior. Since the isolated global packages refactor in pnpm 11, the global list command had a custom path that always printed plain text and ignored `--json`/`--parseable`, which broke tools like `npm-check-updates` that parse the JSON output [#11440](https://github.com/pnpm/pnpm/issues/11440).

  `pnpm -g ls --depth=<n>` (with n > 0) now errors when more than one isolated global install would be involved, since each install has its own lockfile and merging their transitive trees would be incoherent. When the request can be narrowed to a single install group, the regular `list` flow is used and the full dependency tree is shown.

- Fixed `pnpm publish` to honor `publishConfig.registry` from `package.json` when publishing a single package. The native publish flow introduced in v11 was reading the registry from `.npmrc` only, ignoring the per-package override [#11419](https://github.com/pnpm/pnpm/issues/11419).
- When `strictPeerDependencies` is `true`, the `ERR_PNPM_PEER_DEP_ISSUES` error once again renders the peer dependency issues inline using the same format as `pnpm peers check`, so users (and CI tools like Renovate) can see what failed without running `pnpm peers check` separately [#11439](https://github.com/pnpm/pnpm/issues/11439).
- The `WARN` and error code labels in pnpm's output now wrap in brackets (`[WARN]`, `[ERR_PNPM_FOO]`). Previously the labels relied entirely on a colored background to stand out, which meant they blended into the surrounding text in terminals without color (e.g. when `NO_COLOR` is set or output is piped). The brackets are painted in the same color as the badge background, so they appear as ordinary padding in color-capable terminals — only the no-color rendering changes.

## 11.0.4

### Patch Changes

- Fixed `pnpm ci` not reinstalling workspace package `node_modules` directories after the clean step [#11427](https://github.com/pnpm/pnpm/issues/11427).
- Remove pnpm's workspace state file when cleaning node_modules so `pnpm ci` performs a fresh install after the clean step.
- Do not remove `pnpm-lock.yaml` during `pnpm clean` when `lockfile: true` is configured in `pnpm-workspace.yaml`. The lockfile is only removed when the `--lockfile` option is passed to `pnpm clean`.
- `pnpm self-update` (with no version argument) no longer downgrades pnpm when the registry's `latest` dist-tag points to an older release than the currently active version. Run `pnpm self-update latest` to force a downgrade [#11418](https://github.com/pnpm/pnpm/issues/11418).
- `minimumReleaseAgeStrict` now defaults to `true` whenever the user explicitly sets `minimumReleaseAge` (via `pnpm-workspace.yaml`, the global `config.yaml`, the CLI, or `pnpm_config_*` env vars).

## 11.0.3

### Patch Changes

- Fix too many open files error sometimes happening on Windows, when creating command shims in `node_modules/.bin` [#11412](https://github.com/pnpm/pnpm/issues/11412).
- Fix `ERR_PNPM_FETCH_404` when installing a project whose lockfile depends on a `file:` tarball. The previous behavior dropped the `tarball` field from `file:` and git-hosted resolutions when `lockfile-include-tarball-url=false` (the default), even though those URLs cannot be reconstructed from the package name, version, and registry [#11407](https://github.com/pnpm/pnpm/issues/11407).

## 11.0.2

### Patch Changes

- Fix `ENOENT` symlink failure when `pnpm add -g` triggers the approve-builds prompt. The global add flow used to forward an absolute `modulesDir` (`<installDir>/node_modules`) into the install run by `approve-builds`. The install layer treated `modulesDir` as a path relative to `lockfileDir` and joined it again, producing a doubled path on Windows because `path.join` does not collapse an embedded absolute path. The hoist step then tried to `mkdir` and symlink under `<installDir>\<installDir>\node_modules\.pnpm\node_modules\...` and failed with `ENOENT` [#11403](https://github.com/pnpm/pnpm/issues/11403).
- Fixed `packageManagerDependencies` going stale when pnpm is invoked through corepack. The lockfile sync (and the `devEngines.packageManager` version check) previously ran only when pnpm was invoked directly; under corepack the entire block was skipped, so a stale entry would persist even after the running pnpm version changed. The lockfile sync now runs regardless of how pnpm was invoked, while the pnpm-managed version switch (`onFail: 'download'`) remains skipped under corepack so it doesn't fight corepack's own version selection [#11397](https://github.com/pnpm/pnpm/issues/11397).
- Fix recursive publish summaries to report the manifest from `publishConfig.directory` when packages publish from a generated directory [#11239](https://github.com/pnpm/pnpm/issues/11239).
- Fix negated `os` / `cpu` entries (e.g. `["!win32"]`) being incorrectly rejected when `supportedArchitectures` expands to multiple platforms [#11375](https://github.com/pnpm/pnpm/pull/11375).

## 11.0.1

### Patch Changes

- Report unknown top-level options before falling back to implicit `pnpm run` scripts.
- Reject `null` named catalogs in workspace manifests with `InvalidWorkspaceManifestError` instead of crashing with a raw `TypeError`.
- Populate download location for git-sourced dependencies in SBOM output. Previously `pnpm sbom` emitted `NOASSERTION` (SPDX) and omitted the distribution reference (CycloneDX) for git dependencies. Now emits the git URL with commit hash, e.g. `git+https://github.com/user/repo.git#commit`.
- `pnpm self-update` now keeps `package.json`'s `packageManager` and `devEngines.packageManager` in sync. When the legacy `packageManager` field pins pnpm, both fields are rewritten to the new exact pnpm version on update — `packageManager` to `pnpm@<version>` (without an integrity hash), and `devEngines.packageManager.version` to the same exact `<version>` (dropping any range operator). When only `devEngines.packageManager` is declared, the existing range-preserving behavior is unchanged [#11388](https://github.com/pnpm/pnpm/issues/11388).
- Sort the keys of the overrides object returned by `pnpm audit --fix` so that the log output order matches the order written to `pnpm-workspace.yaml`.
- Update the env lockfile's `packageManagerDependencies` entry when `devEngines.packageManager` declares a pnpm version that the lockfile no longer satisfies. Previously, the stale entry was kept even though the running pnpm matched the declared version, silently breaking the integrity record [#11387](https://github.com/pnpm/pnpm/issues/11387).

## 11.0.0

### Highlights

#### Major

- **Node.js 22+ required** — support for Node 18, 19, 20, and 21 is dropped, pnpm itself is now pure ESM, and the standalone exe requires glibc 2.27.
- **Supply-chain protection on by default** — `minimumReleaseAge` defaults to 1 day (newly published packages are not resolved for 24h) and `blockExoticSubdeps` defaults to `true`.
- **`allowBuilds` replaces the old build-dependency settings** — `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, `ignoredBuiltDependencies`, and `ignoreDepScripts` have been removed.
- **Global installs are isolated and use the global virtual store by default** — each `pnpm add -g` gets its own directory with its own `package.json`, `node_modules`, and lockfile.
- **New SQLite-backed store index** (store v11) with bundled manifests and hex digests, reducing filesystem syscalls and speeding up installation.
- **Native publish flow** — [`pnpm publish`](https://pnpm.io/11.x/cli/publish), [`login`](https://pnpm.io/11.x/cli/login), [`logout`](https://pnpm.io/11.x/cli/logout), [`view`](https://pnpm.io/11.x/cli/view), [`deprecate`](https://pnpm.io/11.x/cli/deprecate), [`unpublish`](https://pnpm.io/11.x/cli/unpublish), [`dist-tag`](https://pnpm.io/11.x/cli/dist-tag), and [`version`](https://pnpm.io/11.x/cli/version) no longer delegate to the npm CLI, and the remaining npm passthrough commands now throw "not implemented".
- **[`pnpm audit`](https://pnpm.io/11.x/cli/audit) uses npm's bulk advisories endpoint** — the legacy `/security/audits` endpoints are gone. CVE-based filtering has been replaced with GHSA-based filtering: migrate `auditConfig.ignoreCves` entries to `auditConfig.ignoreGhsas`.
- **`.npmrc` is auth/registry only** — all other settings must live in `pnpm-workspace.yaml` or the new global `config.yaml`, and environment variables use the `pnpm_config_*` prefix.
- **Runtime installs are slimmer** — installing a Node.js runtime via `node@runtime:<version>` no longer extracts the bundled `npm`, `npx`, and `corepack`, roughly halving the files pnpm has to hash, write, and link.

#### Minor

- **New commands:** [`pnpm ci`](https://pnpm.io/11.x/cli/ci), [`pnpm sbom`](https://pnpm.io/11.x/cli/sbom), [`pnpm clean`](https://pnpm.io/11.x/cli/clean), [`pnpm peers check`](https://pnpm.io/11.x/cli/peers), [`pnpm runtime set`](https://pnpm.io/11.x/cli/runtime), [`pnpm docs`](https://pnpm.io/11.x/cli/docs)/`home`, [`pnpm ping`](https://pnpm.io/11.x/cli/ping), [`pnpm search`](https://pnpm.io/11.x/cli/search), [`pnpm star`](https://pnpm.io/11.x/cli/star)/`unstar`/`stars`, [`pnpm whoami`](https://pnpm.io/11.x/cli/whoami), [`pnpm with`](https://pnpm.io/11.x/cli/with), and [`pnpm pack-app`](https://pnpm.io/11.x/cli/pack-app), plus `pn`/[`pnx`](https://pnpm.io/11.x/cli/pnx) short aliases.
- **ESM pnpmfiles** via `.pnpmfile.mjs`, which takes priority over `.pnpmfile.cjs` when present.
- **[`pnpm audit --fix=update`](https://pnpm.io/11.x/cli/audit)** fixes vulnerabilities by updating packages in the lockfile instead of adding overrides, and `pnpm audit --fix --interactive` lets you select which advisories to fix.
- **[`pnpm pack-app`](https://pnpm.io/11.x/cli/pack-app)** packs a CommonJS entry into a standalone executable for one or more target platforms using Node.js Single Executable Applications.
- **Faster HTTP and I/O** — undici with Happy Eyeballs, direct-to-CAS writes, skipped staging directory, pre-allocated tarball downloads, and an NDJSON metadata cache.

### Major Changes

#### Requirements

- pnpm is now distributed as pure ESM.
- Dropped support for Node.js v18, 19, 20, and 21.
- The standalone exe version of pnpm requires at least glibc 2.27.

#### Security & Build Defaults

- Changed default values: `optimisticRepeatInstall` is now `true`, `verifyDepsBeforeRun` is now `install`, `minimumReleaseAge` is now `1440` (1 day), and `minimumReleaseAgeStrict` is `false`. Newly published packages will not be resolved until they are at least 1 day old. This protects against supply chain attacks by giving the community time to detect and remove compromised versions. To opt out, set `minimumReleaseAge: 0` in `pnpm-workspace.yaml` [#11158](https://github.com/pnpm/pnpm/pull/11158).
- `strictDepBuilds` is `true` by default.
- `blockExoticSubdeps` is `true` by default.
- Removed deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, `ignoredBuiltDependencies`, and `ignoreDepScripts` [#11220](https://github.com/pnpm/pnpm/pull/11220).

  Use the `allowBuilds` setting instead. It is a map where keys are package name patterns and values are booleans:

  - `true` means the package is allowed to run build scripts
  - `false` means the package is explicitly denied from running build scripts

  Same as before, by default, none of the packages in the dependencies are allowed to run scripts. If a package has postinstall scripts and it isn't declared in `allowBuilds`, an error is printed.

  Before:

  ```yaml
  onlyBuiltDependencies:
    - electron
  onlyBuiltDependenciesFile: "allowed-builds.json"
  neverBuiltDependencies:
    - core-js
  ignoredBuiltDependencies:
    - esbuild
  ```

  After:

  ```yaml
  allowBuilds:
    electron: true
    core-js: false
    esbuild: false
  ```

- Removed `allowNonAppliedPatches` in favor of `allowUnusedPatches`.
- Removed `ignorePatchFailures`; patch application failures now throw an error.

#### Store

- Runtime dependencies are always linked from the global virtual store [#10233](https://github.com/pnpm/pnpm/pull/10233).
- Optimized index file format to store the hash algorithm once per file instead of repeating it for every file entry. Each file entry now stores only the hex digest instead of the full integrity string (`<algo>-<digest>`). Using hex format improves performance since file paths in the content-addressable store use hex representation, eliminating base64-to-hex conversion during path lookups.
- Store version bumped to v11.
- The bundled manifest (name, version, bin, engines, scripts, etc.) is now stored directly in the package index file, eliminating the need to read `package.json` from the content-addressable store during resolution and installation. This reduces I/O and speeds up repeat installs [#10473](https://github.com/pnpm/pnpm/pull/10473).
- The package index in the content-addressable store is now backed by SQLite. Instead of individual JSON files under `$STORE/index/`, package metadata is stored in a single SQLite database at `$STORE/index.db` with MessagePack-encoded values. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10500](https://github.com/pnpm/pnpm/pull/10500) [#10826](https://github.com/pnpm/pnpm/issues/10826).

#### Global Packages

- Global installs (`pnpm add -g pkg`) and `pnx` now use the global virtual store by default. Packages are stored at `{storeDir}/links` instead of per-project `.pnpm` directories. This can be disabled by setting `enableGlobalVirtualStore: false` [#10694](https://github.com/pnpm/pnpm/pull/10694).
- Isolated global packages. Each globally installed package (or group of packages installed together) now gets its own isolated installation directory with its own `package.json`, `node_modules/`, and lockfile. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

  Key changes:

  - `pnpm add -g <pkg>` creates an isolated installation in `{pnpmHomeDir}/global/v11/{hash}/`
  - `pnpm remove -g <pkg>` removes the entire installation group containing the package
  - `pnpm update -g [pkg]` re-installs packages in new isolated directories
  - `pnpm list -g` scans isolated directories to show all installed global packages
  - `pnpm install -g` (no args) is no longer supported; use `pnpm add -g <pkg>` instead

- Globally installed binaries are now stored in a `bin` subdirectory of `PNPM_HOME` instead of directly in `PNPM_HOME`. This prevents internal directories like `global/` and `store/` from polluting shell autocompletion when `PNPM_HOME` is on PATH [#10986](https://github.com/pnpm/pnpm/issues/10986). After upgrading, run `pnpm setup` to update your shell configuration.
- Breaking changes to `pnpm link`:

  - `pnpm link <pkg-name>` no longer resolves packages from the global store. Only relative or absolute paths are accepted. For example, use `pnpm link ./foo` instead of `pnpm link foo`.
  - `pnpm link --global` is removed. Use `pnpm add -g .` to register a local package's bins globally.
  - `pnpm link` (no arguments) is removed. Use `pnpm link <dir>` with an explicit path instead.

#### Configuration

- pnpm no longer reads all settings from `.npmrc`. Only auth and registry settings are read from `.npmrc` files. All other settings (like `hoistPattern`, `nodeLinker`, `shamefullyHoist`, etc.) must be configured in `pnpm-workspace.yaml` or the global `~/.config/pnpm/config.yaml` [#11189](https://github.com/pnpm/pnpm/pull/11189).
- Network settings (`httpProxy`, `httpsProxy`, `noProxy`, `localAddress`, `strictSsl`, `gitShallowHosts`) are now written to `config.yaml` (global) or `pnpm-workspace.yaml` (local) instead of `.npmrc`/`auth.ini`. They are still readable from `.npmrc` for easier migration from the npm CLI [#11209](https://github.com/pnpm/pnpm/pull/11209).

  pnpm no longer reads `npm_config_*` environment variables. Use `pnpm_config_*` environment variables instead (e.g., `pnpm_config_registry` instead of `npm_config_registry`).

  pnpm no longer reads the npm global config at `$PREFIX/etc/npmrc`.

  `pnpm login` writes auth tokens to `~/.config/pnpm/auth.ini`.

  New `registries` setting in `pnpm-workspace.yaml`:

  ```yaml
  registries:
    default: https://registry.npmjs.org/
    "@my-org": https://private.example.com/
    "@internal": https://nexus.corp.com/
  ```

  Auth tokens in `~/.npmrc` still work — pnpm continues to read `~/.npmrc` as a fallback for registry authentication. The new `npmrcAuthFile` setting can be used to point to a different file instead of `~/.npmrc`.

- Replace workspace project specific `.npmrc` with `packageConfigs` in `pnpm-workspace.yaml`.

  A workspace manifest with `packageConfigs` looks something like this:

  ```yaml
  # File: pnpm-workspace.yaml
  packages:
    - "packages/project-1"
    - "packages/project-2"
  packageConfigs:
    "project-1":
      saveExact: true
    "project-2":
      savePrefix: "~"
  ```

  Or this:

  ```yaml
  # File: pnpm-workspace.yaml
  packages:
    - "packages/project-1"
    - "packages/project-2"
  packageConfigs:
    - match: ["project-1", "project-2"]
      modulesDir: "node_modules"
      saveExact: true
  ```

- pnpm no longer reads settings from the `pnpm` field of `package.json`. Settings should be defined in `pnpm-workspace.yaml` [#10086](https://github.com/pnpm/pnpm/pull/10086).
- `pnpm config get` (without `--json`) no longer prints INI formatted text. Instead, it prints JSON for objects and arrays, and raw strings for strings, numbers, booleans, and nulls. `pnpm config get --json` still prints all types of values as JSON, as before.
- `pnpm config get <array>` now prints a JSON array.
- `pnpm config list` now prints a JSON object instead of INI formatted text.
- `pnpm config list` and `pnpm config get` (without argument) now hide auth-related settings.
- `pnpm config list` and `pnpm config get` (without argument) now show top-level keys as camelCase. Exception: keys that start with `@` or `//` are preserved (their cases don't change).
- `pnpm config get` and `pnpm config list` no longer load non-camelCase options from the workspace manifest (`pnpm-workspace.yaml`).

#### Removed Commands & npm Passthrough

- pnpm no longer falls back to the npm CLI. Commands that were previously passed through to npm (`access`, `bugs`, `docs`, `edit`, `find`, `home`, `issues`, `owner`, `ping`, `prefix`, `profile`, `pkg`, `repo`, `search`, `set-script`, `star`, `stars`, `team`, `token`, `unstar`, `whoami`, `xmas`) and their aliases (`s`, `se`) now throw a "not implemented" error, with a suggestion to use the npm CLI directly [#10642](https://github.com/pnpm/pnpm/pull/10642). Other previously passed-through commands — [`view`](https://pnpm.io/11.x/cli/view) (`info`, `show`, `v`), [`login`](https://pnpm.io/11.x/cli/login) (`adduser`), [`logout`](https://pnpm.io/11.x/cli/logout), [`deprecate`](https://pnpm.io/11.x/cli/deprecate), [`unpublish`](https://pnpm.io/11.x/cli/unpublish), [`dist-tag`](https://pnpm.io/11.x/cli/dist-tag), and [`version`](https://pnpm.io/11.x/cli/version) — have been reimplemented natively in pnpm (see New Commands below).
- [`pnpm publish`](https://pnpm.io/11.x/cli/publish) now works without the `npm` CLI.

  The One-time Password feature now reads from `PNPM_CONFIG_OTP` instead of `NPM_CONFIG_OTP`:

  ```sh
  export PNPM_CONFIG_OTP='<your OTP here>'
  pnpm publish --no-git-checks
  ```

  If the registry requests OTP and the user has not provided it via the `PNPM_CONFIG_OTP` environment variable or the `--otp` flag, pnpm will prompt the user directly for an OTP code.

  If the registry requests web-based authentication, pnpm will print a scannable QR code along with the URL.

  Since the new `pnpm publish` no longer calls `npm publish`, some undocumented features may have been unknowingly dropped. If you rely on a feature that is now gone, please open an issue at <https://github.com/pnpm/pnpm/issues>. In the meantime, you can use `pnpm pack && npm publish *.tgz` as a workaround.

- Removed the `pnpm server` command [#10463](https://github.com/pnpm/pnpm/pull/10463).
- Removed support for the `useNodeVersion` and `executionEnv.nodeVersion` fields. `devEngines.runtime` and `engines.runtime` should be used instead [#10373](https://github.com/pnpm/pnpm/pull/10373).
- Removed support for `hooks.fetchers`. We now have a new API for custom fetchers and resolvers via the `fetchers` field of `pnpmfile`.

#### Lifecycle Scripts

- pnpm no longer populates `npm_config_*` environment variables from the pnpm config during lifecycle scripts. Only well-known `npm_*` env vars are now set, matching Yarn's behavior [#11116](https://github.com/pnpm/pnpm/pull/11116).

#### CLI Output

- Cleaner output for script execution: pnpm now prints `$ command` instead of `> pkg@version stage path\n> command`, and shows project name and path only when running in a different directory. The `$ command` line is printed to stderr to keep stdout clean for piping [#11132](https://github.com/pnpm/pnpm/pull/11132).
- During install, instead of rendering the full peer dependency issues tree, pnpm now suggests running [`pnpm peers check`](https://pnpm.io/11.x/cli/peers) to view the issues [#11133](https://github.com/pnpm/pnpm/pull/11133).

#### Lockfile

- Simplified `patchedDependencies` lockfile format from `Record<string, { path: string, hash: string }>` to `Record<string, string>` (selector to hash). Existing lockfiles with the old format are automatically migrated [#10911](https://github.com/pnpm/pnpm/pull/10911).

#### Other

- The default value of the `type` field in the `package.json` file of the project initialized by `pnpm init` command has been changed to `module`.
- Added support for lowercase options in `pnpm add`: `-d`, `-p`, `-o`, `-e` [#9197](https://github.com/pnpm/pnpm/issues/9197).

  When using the `pnpm add` command only:

  - `-p` is now an alias for `--save-prod` instead of `--parseable`
  - `-d` is now an alias for `--save-dev` instead of `--loglevel=info`

- The root workspace project is no longer excluded when it is explicitly selected via a filter [#10465](https://github.com/pnpm/pnpm/pull/10465).

#### Audit

- [`pnpm audit`](https://pnpm.io/11.x/cli/audit) now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported.

  The bulk endpoint does not return CVE identifiers. CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

  - `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
  - `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
  - GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

  To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.

#### Package Manager Settings

- **Breaking:** removed the `managePackageManagerVersions`, `packageManagerStrict`, and `packageManagerStrictVersion` settings. They existed only to derive the `onFail` behavior for the legacy `packageManager` field, and the `pmOnFail` setting introduced alongside [`pnpm with`](https://pnpm.io/11.x/cli/with) subsumes all three — it directly sets the `onFail` behavior of both `packageManager` and `devEngines.packageManager`. The `COREPACK_ENABLE_STRICT` environment variable is no longer honored (it only gated `packageManagerStrict`); use `pmOnFail` instead.

  Migration:

  | Removed setting                       | Replace with                   |
  | ------------------------------------- | ------------------------------ |
  | `managePackageManagerVersions: true`  | `pmOnFail: download` (default) |
  | `managePackageManagerVersions: false` | `pmOnFail: ignore`             |
  | `packageManagerStrict: false`         | `pmOnFail: warn`               |
  | `packageManagerStrictVersion: true`   | `pmOnFail: error`              |
  | `COREPACK_ENABLE_STRICT=0`            | `pmOnFail: warn`               |

#### Runtime Installs

- Installing a Node.js runtime via `node@runtime:<version>` (including `pnpm env use` and `pnpm runtime set node`) no longer extracts the bundled `npm`, `npx`, and `corepack` from the Node.js archive. This cuts roughly half of the files pnpm has to hash, write to the CAS, and link during installation, making runtime installs noticeably faster. Users who still need `npm` can install it as a separate package.

### Minor Changes

#### New Commands

- Added native [`pnpm view`](https://pnpm.io/11.x/cli/view) (`info`, `show`, `v`) command for viewing package metadata from the registry [#11064](https://github.com/pnpm/pnpm/pull/11064).
- Added [`pnpm login`](https://pnpm.io/11.x/cli/login) (and `pnpm adduser` alias) command for authenticating with npm registries. Supports web-based login with QR code as well as classic username/password login [#11094](https://github.com/pnpm/pnpm/pull/11094).
- Added [`pnpm logout`](https://pnpm.io/11.x/cli/logout) command for logging out of npm registries. Revokes the authentication token on the registry and removes it from the local auth config file [#11213](https://github.com/pnpm/pnpm/pull/11213).
- Added native [`pnpm deprecate`](https://pnpm.io/11.x/cli/deprecate) and `pnpm undeprecate` commands for setting and removing deprecation messages on package versions without delegating to the npm CLI [#11120](https://github.com/pnpm/pnpm/pull/11120).
- Added native [`pnpm unpublish`](https://pnpm.io/11.x/cli/unpublish) command. Supports unpublishing specific versions, version ranges via semver, and entire packages with `--force` [#11128](https://github.com/pnpm/pnpm/pull/11128).
- Added native [`pnpm dist-tag`](https://pnpm.io/11.x/cli/dist-tag) command (`ls`, `add`, `rm` subcommands) [#11218](https://github.com/pnpm/pnpm/pull/11218).
- Added [`pnpm sbom`](https://pnpm.io/11.x/cli/sbom) command for generating Software Bill of Materials in CycloneDX 1.7 and SPDX 2.3 JSON formats [#9088](https://github.com/pnpm/pnpm/issues/9088).
- Added [`pnpm clean`](https://pnpm.io/11.x/cli/clean) command that safely removes `node_modules` directories from all workspace projects [#10707](https://github.com/pnpm/pnpm/issues/10707). Use `--lockfile` to also remove `pnpm-lock.yaml` files.
- Added a new command [`pnpm runtime set <runtime name> <runtime version spec> [-g]`](https://pnpm.io/11.x/cli/runtime) for installing runtimes. Deprecated `pnpm env use` in favor of the new command.
- Added the ability to fix vulnerabilities by updating packages in the lockfile instead of adding overrides. Use [`pnpm audit --fix=update`](https://pnpm.io/11.x/cli/audit) [#10341](https://github.com/pnpm/pnpm/pull/10341).
- Added [`pnpm ci`](https://pnpm.io/11.x/cli/ci) command for clean installs [#6100](https://github.com/pnpm/pnpm/issues/6100). The command runs `pnpm clean` followed by `pnpm install --frozen-lockfile`. Designed for CI/CD environments where reproducible builds are critical. Aliases: `pnpm clean-install`, `pnpm ic`, `pnpm install-clean` [#11003](https://github.com/pnpm/pnpm/pull/11003).
- Added [`pnpm peers check`](https://pnpm.io/11.x/cli/peers) command that checks for unmet and missing peer dependency issues by reading the lockfile [#7087](https://github.com/pnpm/pnpm/issues/7087).
- Implemented the [`version`](https://pnpm.io/11.x/cli/version) command natively in pnpm to support workspaces and `workspace:` protocols correctly. The new command allows bumping package versions (major, minor, patch, etc.) with full workspace support and git integration [#10879](https://github.com/pnpm/pnpm/pull/10879).
- [`pnpm audit --fix`](https://pnpm.io/11.x/cli/audit) now supports a new interactive mode via `--interactive`/`-i`.
- Added the [`pnpm docs`](https://pnpm.io/11.x/cli/docs) command and its alias `pnpm home`. This command opens the package documentation or homepage in the browser. When the package has no valid homepage, it falls back to `https://npmx.dev/package/<name>`.
- Added native [`pnpm ping`](https://pnpm.io/11.x/cli/ping) command to test registry connectivity. Provides a simple way to verify connectivity to the configured registry without requiring external tools.
- Implemented native [`search`](https://pnpm.io/11.x/cli/search) command and its aliases (`s`, `se`, `find`).
- Implemented native [`star`, `unstar`, `stars`](https://pnpm.io/11.x/cli/star), and [`whoami`](https://pnpm.io/11.x/cli/whoami) commands.
- Add [`pnpm with <version|current> <args...>`](https://pnpm.io/11.x/cli/with) command. Runs pnpm at a specific version (or the currently active one) for a single invocation, bypassing the project's `packageManager` and `devEngines.packageManager` pins.
- Added a new [`pnpm pack-app`](https://pnpm.io/11.x/cli/pack-app) command that packs a CommonJS entry file into a standalone executable for one or more target platforms, using the [Node.js Single Executable Applications](https://nodejs.org/api/single-executable-applications.html) API under the hood.

#### Configuration

- Added support for a global YAML config file named `config.yaml`.

  Configuration is now split into two categories:

  - Registry and auth settings, which can be stored in INI files such as the global `rc` file and local `.npmrc`.
  - pnpm-specific settings, which can only be loaded from YAML files such as the global `config.yaml` and local `pnpm-workspace.yaml`.

- Added support for loading environment variables whose names start with `pnpm_config_` into config. These environment variables override settings from `pnpm-workspace.yaml` but not CLI arguments.
- Added support for reading `allowBuilds` from `pnpm-workspace.yaml` in the global package directory for global installs.
- Added support for `pnpm config get globalconfig` to retrieve the global config file path [#9977](https://github.com/pnpm/pnpm/issues/9977).
- Added a new setting `virtualStoreOnly` that populates the virtual store without creating importer symlinks, hoisting, bin links, or running lifecycle scripts. This is useful for pre-populating a store (e.g., in Nix builds) without creating unnecessary project-level artifacts. `pnpm fetch` now uses this mode internally [#10840](https://github.com/pnpm/pnpm/issues/10840).
- Added support for specifying the pnpm version via `devEngines.packageManager` in `package.json`. Unlike the `packageManager` field, this supports version ranges. The resolved version is stored in `pnpm-lock.yaml` and reused if it still satisfies the range [#10932](https://github.com/pnpm/pnpm/pull/10932).
- Added a new `dedupePeers` setting that reduces peer dependency duplication. When enabled, peer dependency suffixes use version-only identifiers (`name@version`) instead of full dep paths, eliminating nested suffixes like `(foo@1.0.0(bar@2.0.0))`. This dramatically reduces the number of package instances in projects with many recursive peer dependencies [#11070](https://github.com/pnpm/pnpm/issues/11070).
- Config dependencies are now installed into the global virtual store (`{storeDir}/links/`) and symlinked into `node_modules/.pnpm-config/`. This allows config dependencies to be shared across projects that use the same store, avoiding redundant fetches and imports [#10910](https://github.com/pnpm/pnpm/pull/10910). Config dependency and package manager integrity info is now stored in `pnpm-lock.yaml` instead of inlined in `pnpm-workspace.yaml`: the workspace manifest contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install [#10912](https://github.com/pnpm/pnpm/pull/10912) [#10964](https://github.com/pnpm/pnpm/pull/10964).
- Added `nodeDownloadMirrors` setting to configure custom Node.js download mirrors in `pnpm-workspace.yaml`. This replaces the `node-mirror:<channel>` `.npmrc` setting, which is no longer read [#11194](https://github.com/pnpm/pnpm/pull/11194):

  ```yaml
  nodeDownloadMirrors:
    release: https://my-mirror.example.com/download/release/
  ```

- `pnpm dlx` and `pnpm create` now respect security and trust policy settings (`minimumReleaseAge`, `minimumReleaseAgeExclude`, `minimumReleaseAgeStrict`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`) from project-level configuration [#11183](https://github.com/pnpm/pnpm/issues/11183).
- `pnpm init` now writes a `devEngines.packageManager` field instead of the `packageManager` field when `init-package-manager` is enabled.
- Added a new setting `runtimeOnFail` that overrides the `onFail` field of `devEngines.runtime` (and `engines.runtime`) in the root project's `package.json`. Accepted values: `ignore`, `warn`, `error`, `download`. For example, setting `runtimeOnFail=download` makes pnpm download the declared runtime version even when the manifest does not set `onFail: "download"`.
- Added a new setting `minimumReleaseAgeIgnoreMissingTime`, which is `true` by default. When enabled, pnpm skips the `minimumReleaseAge` maturity check if the registry metadata does not include the `time` field. Set to `false` to fail resolution instead.

#### Store

- When the global virtual store is enabled, packages that are not allowed to build (and don't transitively depend on packages that are) now get hashes that don't include the engine name (platform, architecture, Node.js major version). This means ~95% of packages in the GVS survive Node.js upgrades and architecture changes without re-import [#10837](https://github.com/pnpm/pnpm/issues/10837).

#### Hooks & Pnpmfiles

- Added support for pnpmfiles written in ESM, using the `.mjs` extension. When `.pnpmfile.mjs` exists, it takes priority over `.pnpmfile.cjs` and only one is loaded [#9730](https://github.com/pnpm/pnpm/pull/9730).

#### CLI & Other

- The built-in `clean`, `setup`, `deploy`, and `rebuild` commands now prefer user scripts over built-in commands. When a project's `package.json` has a script with the same name, `pnpm` executes the script instead of the built-in command. Added `purge` as an alias for the built-in `clean` command, which always runs the built-in regardless of scripts [#11118](https://github.com/pnpm/pnpm/pull/11118).
- Added `-F` as a short alias for the `--filter` option.
- Added support for hidden scripts. Scripts starting with `.` are hidden and cannot be run directly via `pnpm run`. They can only be called from other scripts. Hidden scripts are also omitted from the `pnpm run` listing [#11041](https://github.com/pnpm/pnpm/pull/11041).
- `pnpm approve-builds` now accepts positional arguments for approving or denying packages without the interactive prompt. Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched [#11030](https://github.com/pnpm/pnpm/pull/11030).
- During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added to `pnpm-workspace.yaml` with a placeholder value, so users can manually set them to `true` or `false` [#11030](https://github.com/pnpm/pnpm/pull/11030).
- Added `pn` and `pnx` short aliases for `pnpm` and `pnpx` (`pnpm dlx`) [#11052](https://github.com/pnpm/pnpm/pull/11052).
- `pnpm store prune` now displays the total size of removed files [#11047](https://github.com/pnpm/pnpm/pull/11047).
- `pnpm audit --fix` now adds the minimum patched version for each advisory to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`, so the security fix can be installed without waiting for `minimumReleaseAge` [#11216](https://github.com/pnpm/pnpm/pull/11216).
- pnpm now warns when `optimisticRepeatInstall` skips `shouldRefreshResolution` hooks [#10995](https://github.com/pnpm/pnpm/pull/10995).

#### Performance

- Replaced `node-fetch` with native `undici` for HTTP requests throughout pnpm [#10537](https://github.com/pnpm/pnpm/pull/10537).
- Eliminated redundant internal linking during GVS warm reinstall when no packages were added [#11073](https://github.com/pnpm/pnpm/pull/11073).
- Eliminated the staging directory when importing packages into `node_modules`, avoiding the overhead of creating a temp dir and renaming per package [#11088](https://github.com/pnpm/pnpm/pull/11088).
- CAS files are now written directly to their final content-addressed path instead of to a temp file and renamed. This eliminates ~30k rename syscalls per cold install [#11087](https://github.com/pnpm/pnpm/pull/11087).
- Optimized hot-path string operations in the content-addressable store and increased `gunzipSync` chunk size for fewer buffer allocations during tarball decompression [#11086](https://github.com/pnpm/pnpm/pull/11086).
- Improved HTTP performance with Happy Eyeballs (dual-stack), better keep-alive settings, and an optimized global dispatcher. Tarball downloads with known size now pre-allocate memory to avoid double-copy overhead [#11151](https://github.com/pnpm/pnpm/pull/11151).
- Adopted `If-Modified-Since` for conditional metadata fetches, avoiding re-downloading unchanged registry metadata [#11161](https://github.com/pnpm/pnpm/pull/11161).
- Switched to abbreviated metadata when checking `minimumReleaseAge`, reducing the amount of data fetched from the registry [#11160](https://github.com/pnpm/pnpm/pull/11160).
- Switched the metadata cache to NDJSON format, improving read/write performance [#11188](https://github.com/pnpm/pnpm/pull/11188).

### Patch Changes

- Switched to `process.stderr.write` instead of `console.error` for script logging [#11140](https://github.com/pnpm/pnpm/pull/11140).
- Respected the `frozen-lockfile` flag when migrating config dependencies [#11067](https://github.com/pnpm/pnpm/pull/11067).
- Removed the `--workspace` flag from the `version` command [#11115](https://github.com/pnpm/pnpm/pull/11115).
- Handled `ENOTSUP` error in the clone import path during parallel I/O [#11117](https://github.com/pnpm/pnpm/pull/11117).
- Fixed `pnpm audit` command.
- Updated dependencies to fix vulnerabilities.
- pnpm now checks whether a package is installable for non-npm-hosted packages (e.g., git or tarball dependencies) after the manifest has been fetched.
- pnpm now explicitly passes the path of the global `rc` config file to `npm`.
- Fixed YAML formatting preservation in `pnpm-workspace.yaml` when running commands like `pnpm update`. Previously, quotes and other formatting were lost even when catalog values didn't change.

  Closes #10425

- The parameter set by the `--allow-build` flag is now written to `allowBuilds`.
- Fixed a bug in which specifying `filter` in `pnpm-workspace.yaml` would cause pnpm to not detect any projects.
- Deferred patch errors until all patches in a group are applied, so that one failed patch does not prevent other patches from being attempted.
- pnpm now fails on incompatible lockfiles in CI when frozen lockfile mode is enabled [#10978](https://github.com/pnpm/pnpm/pull/10978).
- Fixed `strictDepBuilds` and `allowBuilds` checks being bypassed when a package's build side-effects are cached in the store [#11039](https://github.com/pnpm/pnpm/pull/11039).
- In GVS mode, `pnpm approve-builds` now runs a full install instead of rebuild, ensuring that GVS hash directories and symlinks are updated correctly after changing `allowBuilds` [#11043](https://github.com/pnpm/pnpm/pull/11043).
- Fixed a crash in the lockfile merger when merging non-semver version strings (e.g. `link:`, `file:`, git URLs) [#11102](https://github.com/pnpm/pnpm/pull/11102).
- Handled `ENOTSUP` error in `linkOrCopy` during parallel imports [#11103](https://github.com/pnpm/pnpm/pull/11103).
- Skipped linking bins that already reference the correct target. This avoids redundant I/O during repeated installs and prevents permission errors when the store is read-only (e.g. Docker layer caching, CI prewarm, NFS) [#11069](https://github.com/pnpm/pnpm/pull/11069).
- Fixed `_password` handling for the default registry to decode from base64 before use, consistent with scoped registry behavior [#11089](https://github.com/pnpm/pnpm/pull/11089).
- Fixed a bug where the CAS locker cache was not updated when a file already existed with correct integrity [#11085](https://github.com/pnpm/pnpm/pull/11085).
- Prevented catalog entries from being removed by `cleanupUnusedCatalogs` when they are referenced only from workspace `overrides` [#11075](https://github.com/pnpm/pnpm/pull/11075).
- Resolved patch file paths during `pnpm fetch` [#11054](https://github.com/pnpm/pnpm/pull/11054).
- Fixed invalid specifiers for peers on all non-exact version selectors [#11049](https://github.com/pnpm/pnpm/pull/11049).
- Fixed false "Command not found" error on Windows when the command exists but exits with a non-zero exit code [#11000](https://github.com/pnpm/pnpm/issues/11000).
- Prepended `Bearer` to the authorization token generated by `tokenHelper` if it is missing, aligning with npm's behavior [#11097](https://github.com/pnpm/pnpm/pull/11097).
- Propagated error cause when throwing `PnpmError` in `@pnpm/npm-resolver` [#10990](https://github.com/pnpm/pnpm/pull/10990).
- Fixed SQLite race condition during store initialization on Windows.
- Removed `rimrafSync` in `importIndexedDir` fast-path error handler [#11168](https://github.com/pnpm/pnpm/pull/11168).
- Fixed `pnpm dedupe --check` unexpectedly failing due to non-deterministic resolution [#11110](https://github.com/pnpm/pnpm/pull/11110).
- Fixed empty files not being rejected in `isEmptyDirOrNothing` [#11182](https://github.com/pnpm/pnpm/pull/11182).
- Fixed `.bat`/`.cmd` token helpers not working on Windows due to missing `shell: true` option.
