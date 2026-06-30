# @pnpm/npm-resolver

## 1102.1.0

### Minor Changes

- bae694f: Some registries generate tarballs on-demand and cannot provide an integrity checksum in their package metadata. In that case pnpm now computes the integrity from the downloaded tarball and stores it in the lockfile, so the entry is verifiable on subsequent installs instead of being written without an integrity (which would fail the next install). This also applies to `--lockfile-only`: the tarball is downloaded so its integrity can be computed. A lockfile entry that is still missing its integrity is rejected as a `ERR_PNPM_MISSING_TARBALL_INTEGRITY` lockfile verification violation (the install fails closed) rather than being silently re-fetched.

### Patch Changes

- fa7004b: The in-memory package metadata cache is now populated on the exact-version disk fast path, so repeated resolutions of the same package within one install no longer re-read and re-parse the on-disk metadata. In large monorepos this brings the time for adding a new package down from minutes to seconds. The in-memory cache key now also includes the registry, so a package of the same name served by two different registries in a single install can no longer share a cache slot and resolve the wrong tarball.
- 852d537: Lockfile verification no longer reports a registry metadata fetch failure (for example a `403`/`401` on a private registry, or a network error) as `ERR_PNPM_TARBALL_URL_MISMATCH`. When the registry can't be reached to verify an entry, the install now aborts with the registry's own fetch error (such as `ERR_PNPM_FETCH_403`, which already explains the authentication situation) instead of mislabeling a transport failure as lockfile tampering. Registry fetch errors no longer leak basic-auth credentials embedded in the registry URL (`https://user:pass@host/`) into their message.
- Updated dependencies [25a829e]
- Updated dependencies [bae694f]
- Updated dependencies [fbdc0eb]
- Updated dependencies [852d537]
  - @pnpm/config.version-policy@1100.1.6
  - @pnpm/resolving.resolver-base@1100.5.0
  - @pnpm/error@1100.0.1
  - @pnpm/store.cafs@1100.1.11
  - @pnpm/resolving.jsr-specifier-parser@1100.0.1
  - @pnpm/store.index@1100.2.1
  - @pnpm/worker@1100.2.2
  - @pnpm/crypto.hash@1100.0.1

## 1102.0.1

### Patch Changes

- 29ab905: Fixed `pnpm update` overriding the version range policy of a named catalog whose name parses as a version (e.g. `catalog:express4-21`). The `catalog:` reference carries no pinning of its own, so the prefix from the catalog entry (such as `~`) is now preserved instead of being widened to `^` [#10321](https://github.com/pnpm/pnpm/issues/10321).
  - @pnpm/worker@1100.2.1

## 1102.0.0

### Patch Changes

- 61810aa: Added a new setting `frozenStore` (`--frozen-store`) that lets `pnpm install` run against a package store on a read-only filesystem (e.g. a Nix store, a read-only bind mount, an OCI layer). When enabled, pnpm opens the store's SQLite `index.db` through the `immutable=1` URI — bypassing the WAL/`-shm` sidecar creation that otherwise fails on a read-only directory — and suppresses every store-write path (the `index.db` writer and the project-registry write). Pair it with `--offline --frozen-lockfile` against a fully-populated store. Under the global virtual store, package directories live inside the store, so if the store is missing the build output of a package whose lifecycle scripts are approved (or that has a patch), pnpm fails up front with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` rather than crashing mid-build on a read-only write — seed the store with those builds first. Incompatible with `--force` and with a configured pnpr server, since both write into the store; the side-effects cache is likewise not written under `frozenStore`. If the store is missing its content directory, the install fails fast with `ERR_PNPM_FROZEN_STORE_INCOMPLETE` rather than attempting to initialize it. The read-only `immutable=1` open requires Node.js >=22.15.0, >=23.11.0, or >=24.0.0; on older runtimes `--frozen-store` fails with a clear `ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE` error. Bin-linking also tolerates a read-only store: under the global virtual store a package's bin source lives inside the store, so the `chmod` that makes it executable would be refused — with `EPERM`/`EACCES`, or with `EROFS` on a genuinely read-only filesystem. That `chmod` is redundant when the seed already ships its bins executable with a normalized shebang, so it is now skipped in that case, while a non-executable bin (or one still carrying a Windows CRLF shebang) on a read-only store still errors.
- 681b593: pnpm can now use different auth tokens for different package scopes, even when those scopes use the same registry URL.

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

- 1310ab5: A `304 Not Modified` answer from the registry now renews the cached metadata file's mtime, so the `minimumReleaseAge` freshness shortcut keeps serving resolutions from the cache. Previously, once a cached packument grew older than `minimumReleaseAge`, every subsequent install re-validated it against the registry forever, because a 304 never rewrites the file.
- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [61810aa]
- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
  - @pnpm/store.index@1100.2.0
  - @pnpm/worker@1100.2.0
  - @pnpm/fetching.types@1100.0.2
  - @pnpm/types@1101.3.2
  - @pnpm/config.version-policy@1100.1.5
  - @pnpm/core-loggers@1100.2.1
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.9
  - @pnpm/store.cafs@1100.1.10
  - @pnpm/workspace.range-resolver@1100.0.2
  - @pnpm/config.pick-registry-for-package@1100.0.9
  - @pnpm/resolving.registry.types@1100.1.3
  - @pnpm/resolving.resolver-base@1100.4.2
  - @pnpm/crypto.hash@1100.0.1

## 1101.5.2

### Patch Changes

- Updated dependencies [f11b4fc]
  - @pnpm/core-loggers@1100.2.0
  - @pnpm/worker@1100.1.11
  - @pnpm/crypto.hash@1100.0.1

## 1101.5.1

### Patch Changes

- Updated dependencies [089484a]
- Updated dependencies [bf1b731]
  - @pnpm/worker@1100.1.10
  - @pnpm/types@1101.3.1
  - @pnpm/config.pick-registry-for-package@1100.0.8
  - @pnpm/config.version-policy@1100.1.4
  - @pnpm/core-loggers@1100.1.4
  - @pnpm/resolving.registry.types@1100.1.2
  - @pnpm/resolving.resolver-base@1100.4.1
  - @pnpm/store.cafs@1100.1.9
  - @pnpm/crypto.hash@1100.0.1
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.8

## 1101.5.0

### Minor Changes

- 6d17b66: The lockfile verifier now checks that a registry entry pinning an explicit `tarball` URL points at the artifact the registry's own metadata lists for that `name@version`. Previously a tampered lockfile could pair a trusted `name@version` with an attacker-chosen tarball URL (and a matching integrity for those bytes), so the install fetched the attacker's bytes. A mismatch — or any entry that can't be confirmed against the registry — is rejected with `ERR_PNPM_TARBALL_URL_MISMATCH`. Non-registry resolutions (`file:`, git-hosted, etc.) and registry entries without an explicit tarball URL (the URL is reconstructed from name+version+registry, so it is inherently bound) are unaffected; non-standard registry tarball URLs (npm Enterprise, GitHub Packages) still pass because they match the metadata.

  This binding is unconditional — it runs regardless of `minimumReleaseAge`/`trustPolicy` and is not narrowed by their exclude lists, since it guards integrity rather than maturity/trust. It is **fail-closed**: an entry passes only when the registry metadata affirmatively lists the version with a matching tarball URL. If the metadata can't be fetched, doesn't list the version, or omits `dist.tarball`, the entry is rejected. As a result, an install that re-verifies a lockfile (any install whose lockfile content changed since the last verified run, where the verification cache no longer applies) now requires the configured registry to be reachable. `trustLockfile` is the opt-out for environments that treat the on-disk lockfile as already trusted.

  The `minimumReleaseAge`/`trustPolicy` verification also no longer applies to URL-keyed tarball dependencies (e.g. `https:` tarballs) that carry a semver `version` copied from their manifest — those are deliberate non-registry dependencies.

### Patch Changes

- 722b9cd: Skip lockfile `minimumReleaseAge`/`trustPolicy` verification for non-registry tarball protocols (for example `file:`), so local tarball dependencies are not incorrectly checked against npm registry metadata.
- Updated dependencies [3b76b8e]
- Updated dependencies [a017bf3]
- Updated dependencies [6d17b66]
  - @pnpm/worker@1100.1.9
  - @pnpm/types@1101.3.0
  - @pnpm/resolving.resolver-base@1100.4.0
  - @pnpm/config.pick-registry-for-package@1100.0.7
  - @pnpm/config.version-policy@1100.1.3
  - @pnpm/core-loggers@1100.1.3
  - @pnpm/resolving.registry.types@1100.1.1
  - @pnpm/store.cafs@1100.1.8
  - @pnpm/crypto.hash@1100.0.1
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.7

## 1101.4.0

### Minor Changes

- 1e9ab29: Staged publishes are now recognized in the trust scale. When a package version's registry metadata carries an `approver` field, it is treated as the strongest trust evidence (ranked above trusted publishers and provenance attestations), since staged publishes require 2FA publish approvals. This prevents false-positive trust downgrade errors when moving from a staged publish to a lower trust level [#11887](https://github.com/pnpm/pnpm/issues/11887).

### Patch Changes

- 6235428: Fix `minimumReleaseAgeExclude` handling in npm resolution fast paths so excluded packages do not get pinned to stale versions. Excludes are honored consistently during `publishedBy` metadata selection and cache-mtime shortcuts.
- Updated dependencies [1e9ab29]
  - @pnpm/resolving.registry.types@1100.1.0
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.6
  - @pnpm/crypto.hash@1100.0.1

## 1101.3.3

### Patch Changes

- 0721d64: Require provenance before treating trusted publisher metadata as the strongest trust evidence.
- Updated dependencies [aa6149d]
- Updated dependencies [35d2355]
  - @pnpm/worker@1100.1.8
  - @pnpm/types@1101.2.0
  - @pnpm/config.pick-registry-for-package@1100.0.6
  - @pnpm/config.version-policy@1100.1.2
  - @pnpm/core-loggers@1100.1.2
  - @pnpm/resolving.registry.types@1100.0.5
  - @pnpm/resolving.resolver-base@1100.3.1
  - @pnpm/store.cafs@1100.1.7
  - @pnpm/crypto.hash@1100.0.1
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.5

## 1101.3.2

### Patch Changes

- 212315d: Added a new setting `trustLockfile`. When `true`, `pnpm install` skips the supply-chain verification pass that re-applies `minimumReleaseAge` / `trustPolicy='no-downgrade'` to every entry in the loaded lockfile. The install treats the lockfile as already-trusted — useful for closed-source projects where every commit comes from a trusted author, or for CI runs against an already-verified lockfile. Defaults to `false`; verification stays on by default. Set in `pnpm-workspace.yaml`.

  Also cut the memory footprint of the verification pass itself: the per-(registry, name) trust-meta cache previously retained the full packument — dependency graphs, scripts, README, and per-version manifests — for the entire install. On large workspaces (`~4k` lockfile entries with `minimumReleaseAge` + `trustPolicy: no-downgrade` enabled) this could OOM CI runners with a 2GB heap cap. The cache now stores only the fields the trust check actually reads (`time`, per-version `_npmUser.trustedPublisher`, `dist.attestations.provenance`). The abbreviated-metadata cache is similarly projected to just the package-level `modified` field and the set of currently-listed version names. Fixes [#11860](https://github.com/pnpm/pnpm/issues/11860).

## 1101.3.1

### Patch Changes

- Updated dependencies [097983f]
  - @pnpm/config.pick-registry-for-package@1100.0.5

## 1101.3.0

### Minor Changes

- 1627943: `pnpm outdated` and `pnpm update --interactive` now report Node.js, Deno, and Bun runtimes installed as project dependencies (`runtime:` specifiers). Previously these were silently skipped because the npm specifier parser did not understand the `runtime:` protocol, so runtime versions never appeared in the outdated table or the interactive update picker.

  Internally, the outdated check is now resolver-driven: `@pnpm/resolving.resolver-base` defines a `ResolveLatestFunction` shape (with `LatestQuery` input — `{ wantedDependency, compatible? }` — and `LatestInfo` result — `{ latestManifest? }`), and every protocol resolver (npm, jsr, named-registry, git, tarball, local, node/bun/deno runtimes) exports its own `resolveLatest*` function alongside its `resolve*`. `@pnpm/resolving.default-resolver` composes them into a single dispatcher, exposed through `@pnpm/installing.client` as `createResolver(...).resolveLatest`.

  Each resolver decides whether it owns the dep and what "latest" means for its protocol; the outdated command derives `current` / `wanted` display values from the lockfile snapshot (`pkgSnapshot.version` for semver protocols, raw ref for URL-shaped ones) and uses raw ref equality for the "lockfile changed" check, so protocol knowledge stays inside each resolver instead of the command.

### Patch Changes

- 3a54205: Fix the `minimumReleaseAge` (publishedBy) maturity shortcut to be inclusive at the cutoff. Previously, abbreviated metadata whose `modified` field equalled the cutoff fell off the fast path and triggered a full-metadata re-fetch (or a `MISSING_TIME` error when full metadata wasn't permitted). Since `modified` is an upper bound on every version's publish time, `modified == publishedBy` already implies every version passes the per-version `<=` filter in `filterPkgMetadataByPublishDate`, so the shortcut now accepts the boundary case directly. Strictly `>` (was `>=`) at the rejection branch.
- Updated dependencies [1627943]
- Updated dependencies [64afc92]
  - @pnpm/resolving.resolver-base@1100.3.0
  - @pnpm/types@1101.1.1
  - @pnpm/config.pick-registry-for-package@1100.0.4
  - @pnpm/config.version-policy@1100.1.1
  - @pnpm/core-loggers@1100.1.1
  - @pnpm/resolving.registry.types@1100.0.4
  - @pnpm/store.cafs@1100.1.6
  - @pnpm/worker@1100.1.7
  - @pnpm/crypto.hash@1100.0.1
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.4

## 1101.2.0

### Minor Changes

- 963861c: Sped up the `minimumReleaseAge` lockfile verification gate on cold-cache installs by trying npm's `/-/npm/v1/attestations/<name>@<version>` endpoint before fetching the full metadata document. The attestation response is tens of KB versus the multi-MB full metadata, so `--frozen-lockfile` installs against a fleet of provenance-published packages download far less to verify timestamps.

  The publish time comes from `bundle.verificationMaterial.tlogEntries[].integratedTime` (the Rekor inclusion time, a couple of seconds after the actual publish — close enough for a policy that operates in minutes/hours/days). When the local full-metadata mirror already has the timestamp, or the attestation endpoint 404s / errors, the verifier falls back to the existing `fetchFullMetadataCached` path. Sigstore signature verification is not performed; the trust model is unchanged versus reading the registry's `time` field on the full metadata document [#11687](https://github.com/pnpm/pnpm/issues/11687).

- 4195766: Tightened the `minimumReleaseAge` story so the bypass becomes explicit on disk instead of silent, and removed the discover-by-loop dance for strict-mode users:

  1. Fresh resolutions in loose mode (`minimumReleaseAgeStrict: false`) that fall back to a version newer than the cutoff auto-collect the picked `name@version` into the workspace manifest's `minimumReleaseAgeExclude`. A single info message lists the additions; entries already on the list are left alone.
  2. The post-resolution lockfile verifier introduced in #11583 now runs in loose mode too — every accepted-immature pin must be on `minimumReleaseAgeExclude`, just like strict mode requires. A lockfile produced under a weaker (or absent) policy that still has immature entries is rejected the same way strict mode would reject it.
  3. **Strict mode (interactive)** no longer aborts on the first immature pick. The resolver gathers every immature direct _and_ transitive in one pass; before peer-dependency resolution runs, pnpm prompts the user with the full list and asks whether to add them all to `minimumReleaseAgeExclude` and proceed. Approve → install continues and the workspace manifest is written at the end. Decline → resolution aborts before the lockfile or package.json is touched (tarballs already in the store stay, since the store is idempotent). This closes the [#10488](https://github.com/pnpm/pnpm/issues/10488) loop where security bumps to packages with platform-specific transitives (e.g. `next` + the `@next/swc-*` shims) made users re-run `pnpm add` once per transitive.
  4. **Strict mode (non-interactive / CI)** now aborts with the full immature set in the error message instead of the first pick. The resolver always collects every immature direct + transitive; the install command then throws `ERR_PNPM_NO_MATURE_MATCHING_VERSION` listing each entry's `name@version` and publish time. Deterministic CI behavior is preserved (same exit code, same error code), but the error pinpoints every offending entry instead of forcing the discover-by-loop dance. The expected workflow is interactive approval locally → the lockfile + workspace manifest get committed → CI runs cleanly against the populated exclude list.

  5. **The lockfile verifier now also covers `trustPolicy: 'no-downgrade'`.** The same post-resolution gate that re-checks `minimumReleaseAge` on lockfile entries now re-runs `failIfTrustDowngraded` for every npm-registry entry whose name isn't on `trustPolicyExclude`. The two checks share a single full-metadata fetch per package, so the extra coverage doesn't cost an extra round trip when both policies are active. Resolver-time trust checks still run as before — this just closes the gap when an entry bypasses resolution (peek path, `--frozen-lockfile`, restored CI cache).

  Pacquet parity: not ported — pacquet's `minimumReleaseAge` policy is itself only stubbed today (see `pacquet/crates/package-manager/src/version_policy.rs`). The auto-exclude, loose-mode verifier, prompt, and the new trust-policy verifier check will travel with the broader policy port whenever that happens.

- 31538bf: Restructured the `minimumReleaseAge` lockfile revalidation gate around a generic `ResolutionVerifier` interface. Each resolver may now export a sibling verifier factory (today: `createNpmResolutionVerifier`) that re-checks an already-resolved lockfile entry against its policies; the resolver chain returns the verifier list as `resolutionVerifiers` and the install side fans out across it. A `ResolutionVerifier` carries `verify` plus `policy` and `canTrustPastCheck` — the cache contract that lets repeat installs against an unchanged lockfile skip the per-package registry round trip entirely.

  Verification results are memoized in JSON Lines at `<cacheDir>/lockfile-verified.jsonl`: a stat-only fast path matches on lockfile size, mtime, and inode, falling back to a content hash when those drift (typical after a CI checkout). Every active verifier's policy contribution is merged into a single `policy` bag on the record; the gate runs in full whenever the lockfile changes, any verifier rejects the cached policy, or no record exists [#11687](https://github.com/pnpm/pnpm/issues/11687).

### Patch Changes

- Updated dependencies [4195766]
- Updated dependencies [31538bf]
- Updated dependencies [b6e2c8c]
- Updated dependencies [4a79336]
  - @pnpm/resolving.resolver-base@1100.2.0
  - @pnpm/config.version-policy@1100.1.0
  - @pnpm/core-loggers@1100.1.0
  - @pnpm/store.cafs@1100.1.5
  - @pnpm/worker@1100.1.6
  - @pnpm/crypto.hash@1100.0.1

## 1101.1.1

### Patch Changes

- 50b33c1: Address CodeQL static-analysis findings: guard manifest dependency writes against prototype-polluting keys (`__proto__`, `constructor`, `prototype`), and replace a potentially super-linear semver-detection regex in registry 404 hints with an O(n) parser.
- e526f89: Fix `minimumReleaseAge` handling for cached abbreviated metadata.

  The version-spec cache fast path no longer rethrows `ERR_PNPM_MISSING_TIME` under `strictPublishedByCheck`; it now falls through to the registry-fetch path, consistent with the adjacent mtime-gated cache block.

  When the registry returns 304 Not Modified for a package whose cached metadata is abbreviated (no per-version `time`), pnpm now re-fetches with `fullMetadata: true` if `minimumReleaseAge` is active and the package was modified after the cutoff. The upgraded metadata is persisted to disk so subsequent installs don't repeat the fetch. Previously the abbreviated meta was used as-is and the maturity check fell back to its warn-and-skip path, silently bypassing the quarantine and emitting a misleading "metadata is missing the time field" warning.

  Closes #11619.

- c2c2890: Fix `minimumReleaseAge` / `resolutionMode: time-based` installs failing on lockfiles whose `time:` block is missing entries. The npm-resolver's peek-from-store fast path now surfaces `publishedAt` from the lockfile rather than discarding it, and falls through to a registry metadata fetch when the time-based cutoff can't be computed from the data on hand.
  - @pnpm/store.cafs@1100.1.4
  - @pnpm/worker@1100.1.5
  - @pnpm/crypto.hash@1100.0.1

## 1101.1.0

### Minor Changes

- b61e268: Added support for installing packages from the [GitHub Packages npm registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry) via a built-in `gh:` prefix (e.g. `pnpm add gh:@acme/private`), and, more broadly, for arbitrary named registries in the style of [vlt's named-registry aliases](https://docs.vlt.sh/cli/registries). Authentication is picked up from the existing per-URL `.npmrc` entries (e.g. `//npm.pkg.github.com/:_authToken=...`), so no separate auth mechanism is required.

  Additional aliases — or an override for the built-in `gh` alias, for GitHub Enterprise Server — can be configured under `namedRegistries` in `pnpm-workspace.yaml`:

  ```yaml
  namedRegistries:
    gh: https://npm.pkg.github.example.com/
    work: https://npm.work.example.com/
  ```

  With this, `work:@corp/lib@^2.0.0` resolves against `https://npm.work.example.com/`. [#8941](https://github.com/pnpm/pnpm/issues/8941).

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0
  - @pnpm/config.pick-registry-for-package@1100.0.3
  - @pnpm/core-loggers@1100.0.2
  - @pnpm/resolving.registry.types@1100.0.3
  - @pnpm/resolving.resolver-base@1100.1.3
  - @pnpm/store.cafs@1100.1.3
  - @pnpm/worker@1100.1.4
  - @pnpm/crypto.hash@1100.0.1
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.3

## 1101.0.3

### Patch Changes

- 15e9e35: Upgrade `@pnpm/semver-diff`, `@pnpm/colorize-semver-diff`, `@pnpm/exec`, and `parse-npm-tarball-url` to versions that expose their helpers as named exports instead of CommonJS default exports. This eliminates the `.default` property accesses that broke under Node.js ESM interop in tests and could fail at runtime in some module loaders.
- Updated dependencies [0c67cb5]
  - @pnpm/store.index@1100.1.0
  - @pnpm/worker@1100.1.3
  - @pnpm/crypto.hash@1100.0.1

## 1101.0.2

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/resolving.resolver-base@1100.1.2
  - @pnpm/store.cafs@1100.1.2
  - @pnpm/crypto.hash@1100.0.1
  - @pnpm/worker@1100.1.2

## 1101.0.1

### Patch Changes

- 184ce26: Fix the package name in README.md.
- Updated dependencies [184ce26]
- Updated dependencies [5a901e7]
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.2
  - @pnpm/config.pick-registry-for-package@1100.0.2
  - @pnpm/resolving.registry.types@1100.0.2
  - @pnpm/workspace.range-resolver@1100.0.1
  - @pnpm/resolving.resolver-base@1100.1.1
  - @pnpm/fetching.types@1100.0.1
  - @pnpm/fs.graceful-fs@1100.1.0
  - @pnpm/worker@1100.1.1
  - @pnpm/store.cafs@1100.1.1
  - @pnpm/crypto.hash@1100.0.1

## 1101.0.0

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/store.cafs@1100.1.0
  - @pnpm/worker@1100.1.0
  - @pnpm/crypto.hash@1100.0.0

## 1100.1.0

### Minor Changes

- 9e0833c: Added a new setting `minimumReleaseAgeIgnoreMissingTime`, which is `true` by default. When enabled, pnpm skips the `minimumReleaseAge` maturity check if the registry metadata does not include the `time` field. Set to `false` to fail resolution instead.

### Patch Changes

- Updated dependencies [72c1e05]
  - @pnpm/resolving.resolver-base@1100.1.0
  - @pnpm/store.cafs@1100.0.2
  - @pnpm/worker@1100.0.2
  - @pnpm/crypto.hash@1100.0.0

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/config.pick-registry-for-package@1100.0.1
  - @pnpm/core-loggers@1100.0.1
  - @pnpm/resolving.registry.types@1100.0.1
  - @pnpm/resolving.resolver-base@1100.0.1
  - @pnpm/store.cafs@1100.0.1
  - @pnpm/worker@1100.0.1
  - @pnpm/crypto.hash@1100.0.0
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.1

## 1005.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 19f36cf: Changed the error code for no matching version that satisfies the maturity configuration.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- 56a59df: Store the bundled manifest (name, version, bin, engines, scripts, etc.) directly in the package index file, eliminating the need to read `package.json` from the content-addressable store during resolution and installation. This reduces I/O and speeds up repeat installs [#10473](https://github.com/pnpm/pnpm/pull/10473).

### Minor Changes

- facdd71: Adding `trustPolicyIgnoreAfter` allows you to ignore trust policy checks for packages published more than a specified time ago[#10352](https://github.com/pnpm/pnpm/issues/10352).
- 0625e20: Support bare `workspace:` protocol without version specifier. It is now treated as `workspace:*` and resolves to the concrete version during publish [#10436](https://github.com/pnpm/pnpm/pull/10436).
- 10bc391: Added a new setting: `trustPolicy`.
- 15549a9: Add the ability to fix vulnerabilities by updating packages in the lockfile instead of adding overrides.
- 9d3f00b: Added support for `trustPolicyExclude` [#10164](https://github.com/pnpm/pnpm/issues/10164).

  You can now list one or more specific packages or versions that pnpm should allow to install, even if those packages don't satisfy the trust policy requirement. For example:

  ```yaml
  trustPolicy: no-downgrade
  trustPolicyExclude:
    - chokidar@4.0.3
    - webpack@4.47.0 || 5.102.1
  ```

### Patch Changes

- a297ebc: Improve error message when a package version exists but does not meet the `minimumReleaseAge` constraint. The error now clearly states that the version exists and shows a human-readable time since release (e.g., "released 6 hours ago") [#10307](https://github.com/pnpm/pnpm/issues/10307).
- 831f574: When package metadata is malformed or can't be fetched, the error thrown will now show the originating error.
- 0e9c559: An internal refactor was performed to remove a misleading usage of `pMemoize`. Previously the `maxAge` argument was passed, but this field is ignored by the `p-memoize` NPM package.
- 19f36cf: Don't silently skip an optional dependency if it cannot be resolved from a version that satisfies the `minimumReleaseAge` setting [#10270](https://github.com/pnpm/pnpm/issues/10270).
- 61cad0c: fix: treat HTTP 400 responses as errors in the npm resolver fetch

  The status check used `> 400` instead of `>= 400`, causing 400 Bad Request responses to bypass the error path and fall into JSON parse/retry logic instead.

- 143ca78: Fix `link-workspace-packages=true` incorrectly linking workspace packages when the requested version doesn't match the workspace package's version. Previously, on fresh installs the version constraint is overridden to `*` in the fallback resolution paths, causing any workspace package with a matching name to be linked regardless of version [#10173](https://github.com/pnpm/pnpm/issues/10173).
- 6f361aa: `trustPolicy` should ignore the trust evidences of prerelease versions, when installing a non-prerelease version.
- 938ea1f: Revert Try to avoid making network calls with preferOffline [#10334](https://github.com/pnpm/pnpm/pull/10334).
- 2cb0657: Don't fail with a `ERR_PNPM_MISSING_TIME` error if a package that is excluded from trust policy checks is missing the time field in the metadata.
- bb8baa7: Fixed optional dependencies to request full metadata from the registry to get the `libc` field, which is required for proper platform compatibility checks [#9950](https://github.com/pnpm/pnpm/issues/9950).
- 144ce0e: Improve the error messages related to `trustPolicy` mismatch.
- ba70035: Update parse-npm-tarball-url to fix deprecation warnings on Node.js 24.
- 3585d9a: Normalize the tarball URLs before saving them to the lockfile. URLs should not contain default ports, like :80 for http and :443 for https [#10273](https://github.com/pnpm/pnpm/pull/10273).
- 6557dc0: Fixed a bug preventing the `clearCache` function returned by `createNpmResolver` from properly clearing metadata cache.
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [c55c614]
- Updated dependencies [9b0a460]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [7cec347]
- Updated dependencies [3bf5e21]
- Updated dependencies [491a84f]
- Updated dependencies [6656baa]
- Updated dependencies [2ea6463]
- Updated dependencies [50fbeca]
- Updated dependencies [caabba4]
- Updated dependencies [075aa99]
- Updated dependencies [3bf5e21]
- Updated dependencies [d3d6938]
- Updated dependencies [0625e20]
- Updated dependencies [bb8baa7]
- Updated dependencies [878a773]
- Updated dependencies [f8e6774]
- Updated dependencies [ee9fe58]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [780af09]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [6c480a4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [4893853]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [9d3f00b]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
  - @pnpm/resolving.resolver-base@1006.0.0
  - @pnpm/store.cafs@1001.0.0
  - @pnpm/worker@1001.0.0
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/workspace.range-resolver@1001.0.0
  - @pnpm/config.pick-registry-for-package@1001.0.0
  - @pnpm/resolving.jsr-specifier-parser@1001.0.0
  - @pnpm/fetching.types@1001.0.0
  - @pnpm/core-loggers@1002.0.0
  - @pnpm/workspace.spec-parser@1001.0.0
  - @pnpm/fs.graceful-fs@1001.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/crypto.hash@1001.0.0
  - @pnpm/resolving.registry.types@1000.1.0
  - @pnpm/store.index@1000.0.0
  - @pnpm/resolving.registry.pkg-metadata-filter@1000.1.2

## 1004.4.1

### Patch Changes

- 6c3dcb8: Skip time field validation for packages excluded by `minimumReleaseAgeExclude` (allows packages that would otherwise throw `ERR_PNPM_MISSING_TIME`).
- Updated dependencies [0152a51]
  - @pnpm/registry.pkg-metadata-filter@1000.1.1

## 1004.4.0

### Minor Changes

- 7c1382f: The npm resolver supports `publishedByExclude` now.

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [7c1382f]
- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/registry.pkg-metadata-filter@1000.1.0
  - @pnpm/types@1000.9.0
  - @pnpm/resolver-base@1005.1.0
  - @pnpm/pick-registry-for-package@1000.0.11
  - @pnpm/core-loggers@1001.0.4
  - @pnpm/registry.types@1000.0.1
  - @pnpm/crypto.hash@1000.2.1

## 1004.3.0

### Minor Changes

- fb4da0c: Added network performance monitoring to pnpm by implementing warnings for slow network requests, including both metadata fetches and tarball downloads.

  Added configuration options for warning thresholds: `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps`.
  Warning messages are displayed when requests exceed time thresholds or fall below speed minimums

  Related PR: [#10025](https://github.com/pnpm/pnpm/pull/10025).

### Patch Changes

- Updated dependencies [9b9faa5]
- Updated dependencies [4a2d871]
  - @pnpm/graceful-fs@1000.0.1
  - @pnpm/registry.pkg-metadata-filter@1000.0.0
  - @pnpm/registry.types@1000.0.0
  - @pnpm/crypto.hash@1000.2.1

## 1004.2.3

### Patch Changes

- baf8bf6: When a version specifier cannot be resolved because the versions don't satisfy the `minimumReleaseAge` setting, print this information out in the error message [#9974](https://github.com/pnpm/pnpm/pull/9974).
- 702ddb9: When `minimumReleaseAge` is set and the `latest` tag is not mature enough, prefer a non-deprecated version as the new `latest` [#9987](https://github.com/pnpm/pnpm/issues/9987).

## 1004.2.2

### Patch Changes

- 121b44e: Don't ignore the `minimumReleaseAge` check, when the package is requested by exact version and the packument is loaded from cache [#9978](https://github.com/pnpm/pnpm/issues/9978).
- 02f8b69: When `minimumReleaseAge` is set and the active version under a dist-tag is not mature enough, do not downgrade to a prerelease version in case the original version wasn't a prerelease one [#9979](https://github.com/pnpm/pnpm/issues/9979).

## 1004.2.1

### Patch Changes

- Updated dependencies [6365bc4]
  - @pnpm/constants@1001.3.1
  - @pnpm/error@1000.0.5
  - @pnpm/resolving.jsr-specifier-parser@1000.0.3
  - @pnpm/crypto.hash@1000.2.0

## 1004.2.0

### Minor Changes

- 38e2599: There have been several incidents recently where popular packages were successfully attacked. To reduce the risk of installing a compromised version, we are introducing a new setting that delays the installation of newly released dependencies. In most cases, such attacks are discovered quickly and the malicious versions are removed from the registry within an hour.

  The new setting is called `minimumReleaseAge`. It specifies the number of minutes that must pass after a version is published before pnpm will install it. For example, setting `minimumReleaseAge: 1440` ensures that only packages released at least one day ago can be installed.

  If you set `minimumReleaseAge` but need to disable this restriction for certain dependencies, you can list them under the `minimumReleaseAgeExclude` setting. For instance, with the following configuration pnpm will always install the latest version of webpack, regardless of its release time:

  ```yaml
  minimumReleaseAgeExclude:
    - webpack
  ```

  Related issue: [#9921](https://github.com/pnpm/pnpm/issues/9921).

### Patch Changes

- Updated dependencies [e792927]
  - @pnpm/types@1000.8.0
  - @pnpm/pick-registry-for-package@1000.0.10
  - @pnpm/core-loggers@1001.0.3
  - @pnpm/resolver-base@1005.0.1
  - @pnpm/crypto.hash@1000.2.0

## 1004.1.3

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/constants@1001.3.0
  - @pnpm/resolver-base@1005.0.0
  - @pnpm/error@1000.0.4
  - @pnpm/resolving.jsr-specifier-parser@1000.0.2
  - @pnpm/crypto.hash@1000.2.0

## 1004.1.2

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1ba2e15]
- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/fetching-types@1000.2.0
  - @pnpm/resolver-base@1004.1.0
  - @pnpm/constants@1001.2.0
  - @pnpm/pick-registry-for-package@1000.0.9
  - @pnpm/core-loggers@1001.0.2
  - @pnpm/error@1000.0.3
  - @pnpm/crypto.hash@1000.2.0
  - @pnpm/resolving.jsr-specifier-parser@1000.0.1

## 1004.1.1

### Patch Changes

- Updated dependencies [cf630a8]
  - @pnpm/crypto.hash@1000.2.0

## 1004.1.0

### Minor Changes

- 2721291: Create different resolver result types which provide more information.

### Patch Changes

- Updated dependencies [2721291]
- Updated dependencies [6acf819]
  - @pnpm/resolver-base@1004.0.0
  - @pnpm/crypto.hash@1000.1.1

## 1004.0.1

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [09cf46f]
- Updated dependencies [5ec7255]
  - @pnpm/core-loggers@1001.0.1
  - @pnpm/types@1000.6.0
  - @pnpm/pick-registry-for-package@1000.0.8
  - @pnpm/resolver-base@1003.0.1
  - @pnpm/crypto.hash@1000.1.1

## 1004.0.0

### Major Changes

- 8a9f3a4: `pref` renamed to `bareSpecifier`.
- 5b73df1: Renamed `normalizedPref` to `specifiers`.

### Minor Changes

- 9c3dd03: **Added support for installing JSR packages.** You can now install JSR packages using the following syntax:

  ```
  pnpm add jsr:<pkg_name>
  ```

  or with a version range:

  ```
  pnpm add jsr:<pkg_name>@<range>
  ```

  For example, running:

  ```
  pnpm add jsr:@foo/bar
  ```

  will add the following entry to your `package.json`:

  ```json
  {
    "dependencies": {
      "@foo/bar": "jsr:^0.1.2"
    }
  }
  ```

  When publishing, this entry will be transformed into a format compatible with npm, older versions of Yarn, and previous pnpm versions:

  ```json
  {
    "dependencies": {
      "@foo/bar": "npm:@jsr/foo__bar@^0.1.2"
    }
  }
  ```

  Related issue: [#8941](https://github.com/pnpm/pnpm/issues/8941).

  Note: The `@jsr` scope defaults to <https://npm.jsr.io/> if the `@jsr:registry` setting is not defined.

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/resolver-base@1003.0.0
  - @pnpm/core-loggers@1001.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/resolving.jsr-specifier-parser@1000.0.0
  - @pnpm/types@1000.5.0
  - @pnpm/pick-registry-for-package@1000.0.7
  - @pnpm/crypto.hash@1000.1.1

## 1003.0.0

### Major Changes

- 81f441c: `updateToLatest` replaced with `update` field.

### Patch Changes

- Updated dependencies [81f441c]
  - @pnpm/resolver-base@1002.0.0
  - @pnpm/crypto.hash@1000.1.1

## 1002.0.0

### Major Changes

- 72cff38: The resolving function now takes a `registries` object, so it finds the required registry itself instead of receiving it from package requester.

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [72cff38]
- Updated dependencies [750ae7d]
  - @pnpm/types@1000.4.0
  - @pnpm/resolver-base@1001.0.0
  - @pnpm/core-loggers@1000.2.0
  - @pnpm/pick-registry-for-package@1000.0.6
  - @pnpm/crypto.hash@1000.1.1

## 1001.0.1

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0
  - @pnpm/core-loggers@1000.1.5
  - @pnpm/resolver-base@1000.2.1
  - @pnpm/crypto.hash@1000.1.1

## 1001.0.0

### Major Changes

- 3d52365: The `@pnpm/npm-resolver` package can now return `workspace` in the `resolvedVia` field of its results. This will be the case if the resolved package was requested through the `workspace:` protocol or if the wanted dependency's name and specifier match a package in the workspace. Previously, the `resolvedVia` field was always set to `local-filesystem` for workspace packages.

### Patch Changes

- Updated dependencies [3d52365]
  - @pnpm/resolver-base@1000.2.0
  - @pnpm/crypto.hash@1000.1.1

## 1000.1.7

### Patch Changes

- @pnpm/crypto.hash@1000.1.1

## 1000.1.6

### Patch Changes

- 8371664: When a package version cannot be found in the package metadata, print the registry from which the package was fetched.

## 1000.1.5

### Patch Changes

- Updated dependencies [daf47e9]
- Updated dependencies [a5e4965]
  - @pnpm/crypto.hash@1000.1.0
  - @pnpm/types@1000.2.1
  - @pnpm/core-loggers@1000.1.4
  - @pnpm/resolver-base@1000.1.4

## 1000.1.4

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0
  - @pnpm/core-loggers@1000.1.3
  - @pnpm/resolver-base@1000.1.3
  - @pnpm/crypto.hash@1000.0.0

## 1000.1.3

### Patch Changes

- Updated dependencies [9a44e6c]
- Updated dependencies [b562deb]
  - @pnpm/constants@1001.1.0
  - @pnpm/types@1000.1.1
  - @pnpm/error@1000.0.2
  - @pnpm/core-loggers@1000.1.2
  - @pnpm/resolver-base@1000.1.2
  - @pnpm/crypto.hash@1000.0.0

## 1000.1.2

### Patch Changes

- Updated dependencies [9591a18]
  - @pnpm/types@1000.1.0
  - @pnpm/core-loggers@1000.1.1
  - @pnpm/resolver-base@1000.1.1
  - @pnpm/crypto.hash@1000.0.0

## 1000.1.1

### Patch Changes

- Updated dependencies [516c4b3]
  - @pnpm/core-loggers@1000.1.0
  - @pnpm/crypto.hash@1000.0.0

## 1000.1.0

### Minor Changes

- 6483b64: A new setting, `inject-workspace-packages`, has been added to allow hard-linking all local workspace dependencies instead of symlinking them. Previously, this behavior was achievable via the [`dependenciesMeta[].injected`](https://pnpm.io/package_json#dependenciesmetainjected) setting, which remains supported [#8836](https://github.com/pnpm/pnpm/pull/8836).

### Patch Changes

- Updated dependencies [d2e83b0]
- Updated dependencies [6483b64]
- Updated dependencies [b0f3c71]
- Updated dependencies [a76da0c]
  - @pnpm/constants@1001.0.0
  - @pnpm/resolver-base@1000.1.0
  - @pnpm/fetching-types@1000.1.0
  - @pnpm/error@1000.0.1
  - @pnpm/crypto.hash@1000.0.0

## 22.0.0

### Major Changes

- 501c152: Use SHA256 to encode the package name of a package that has upper case letters in its name.

### Patch Changes

- Updated dependencies [19d5b51]
- Updated dependencies [8108680]
- Updated dependencies [dcd2917]
- Updated dependencies [c4f5231]
  - @pnpm/constants@10.0.0
  - @pnpm/crypto.hash@1.0.0
  - @pnpm/error@6.0.3

## 21.1.1

### Patch Changes

- 222d10a: Use `crypto.hash`, when available, for improved performance [#8629](https://github.com/pnpm/pnpm/pull/8629).
- Updated dependencies [222d10a]
- Updated dependencies [222d10a]
  - @pnpm/crypto.polyfill@1.0.0

## 21.1.0

### Minor Changes

- 83681da: Keep `libc` field in `clearMeta`.

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/constants@9.0.0
  - @pnpm/error@6.0.2

## 21.0.5

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0
  - @pnpm/core-loggers@10.0.7
  - @pnpm/resolver-base@13.0.4

## 21.0.4

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/core-loggers@10.0.6
  - @pnpm/resolver-base@13.0.3

## 21.0.3

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/types@12.0.0
  - @pnpm/core-loggers@10.0.5
  - @pnpm/resolver-base@13.0.2

## 21.0.2

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0
  - @pnpm/core-loggers@10.0.4
  - @pnpm/resolver-base@13.0.1

## 21.0.1

### Patch Changes

- afe520d: Update rename-overwrite to v6.

## 21.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/resolver-base@13.0.0
  - @pnpm/types@11.0.0
  - @pnpm/core-loggers@10.0.3

## 20.0.1

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/core-loggers@10.0.2
  - @pnpm/resolver-base@12.0.2

## 20.0.0

### Major Changes

- 0c08e1c: Breaking change.

## 19.0.4

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/core-loggers@10.0.1
  - @pnpm/resolver-base@12.0.1

## 19.0.3

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 19.0.2

### Patch Changes

- 43b6bb7: Print a better error message when `resolution-mode` is set to `time-based` and the registry fails to return the `"time"` field in the package's metadata.

## 19.0.1

### Patch Changes

- cb0f459: `pnpm update` should not fail when there's an aliased local workspace dependency [#7975](https://github.com/pnpm/pnpm/issues/7975).
- Updated dependencies [cb0f459]
  - @pnpm/workspace.spec-parser@1.0.0

## 19.0.0

### Major Changes

- cdd8365: Package ID does not contain the registry domain.
- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/resolve-workspace-range@6.0.0
  - @pnpm/resolver-base@12.0.0
  - @pnpm/fetching-types@6.0.0
  - @pnpm/core-loggers@10.0.0
  - @pnpm/graceful-fs@4.0.0

## 18.1.0

### Minor Changes

- 31054a63e: Running `pnpm update -r --latest` will no longer downgrade prerelease dependencies [#7436](https://github.com/pnpm/pnpm/issues/7436).

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0

## 18.0.2

### Patch Changes

- 33313d2fd: Update rename-overwrite to v5.
- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/core-loggers@9.0.6
  - @pnpm/resolver-base@11.0.2

## 18.0.1

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/core-loggers@9.0.5
  - @pnpm/resolver-base@11.0.1

## 18.0.0

### Major Changes

- cd4fcfff0: (IMPORTANT) When the package tarballs aren't hosted on the same domain on which the registry (the server with the package metadata) is, the dependency keys in the lockfile should only contain `/<pkg_name>@<pkg_version`, not `<domain>/<pkg_name>@<pkg_version>`.

  This change is a fix to avoid the same package from being added to `node_modules/.pnpm` multiple times. The change to the lockfile is backward compatible, so previous versions of pnpm will work with the fixed lockfile.

  We recommend that all team members update pnpm in order to avoid repeated changes in the lockfile.

  Related PR: [#7318](https://github.com/pnpm/pnpm/pull/7318).

## 17.0.0

### Major Changes

- 4c2450208: (Important) Tarball resolutions in `pnpm-lock.yaml` will no longer contain a `registry` field. This field has been unused for a long time. This change should not cause any issues besides backward compatible modifications to the lockfile [#7262](https://github.com/pnpm/pnpm/pull/7262).

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/resolver-base@11.0.0

## 16.0.13

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/core-loggers@9.0.4
  - @pnpm/resolver-base@10.0.4

## 16.0.12

### Patch Changes

- 01bc58e2c: Update ssri to v10.0.5.
- ff55119a8: Update lru-cache.

## 16.0.11

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/core-loggers@9.0.3
  - @pnpm/resolver-base@10.0.3

## 16.0.10

### Patch Changes

- Updated dependencies [9caa33d53]
  - @pnpm/graceful-fs@3.2.0

## 16.0.9

### Patch Changes

- 41c2b65cf: Respect workspace alias syntax in pkg graph [#6922](https://github.com/pnpm/pnpm/issues/6922)
- Updated dependencies [083bbf590]
  - @pnpm/graceful-fs@3.1.0

## 16.0.8

### Patch Changes

- e958707b2: Improve performance by removing cryptographically generated id from temporary file names.
- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/core-loggers@9.0.2
  - @pnpm/resolver-base@10.0.2

## 16.0.7

### Patch Changes

- @pnpm/error@5.0.2

## 16.0.6

### Patch Changes

- d55b41a8b: Dependencies have been updated.

## 16.0.5

### Patch Changes

- e6052260c: Print a meaningful error when a project referenced by the `workspace:` protocol is not found in the workspace [#4477](https://github.com/pnpm/pnpm/issues/4477).
- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0
  - @pnpm/core-loggers@9.0.1
  - @pnpm/resolver-base@10.0.1
  - @pnpm/error@5.0.1

## 16.0.4

### Patch Changes

- edb3072a9: Update dependencies.

## 16.0.3

### Patch Changes

- c0760128d: bump semver to 7.4.0
- Updated dependencies [c0760128d]
  - @pnpm/resolve-workspace-range@5.0.1

## 16.0.2

### Patch Changes

- ef6c22e12: Improve performance of clean install by preconverting and caching semver objects

## 16.0.1

### Patch Changes

- 642f8c1d0: Reduce max memory usage in npm-resolver

## 16.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.
- 9d026b7cb: Drop node.js 14 support. Update lru-cache.

### Patch Changes

- f835994ea: Deduplicate direct dependencies, when `resolution-mode` is set to `lowest-direct` [#6042](https://github.com/pnpm/pnpm/issues/6042).
- Updated dependencies [eceaa8b8b]
  - @pnpm/resolve-workspace-range@5.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/fetching-types@5.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/graceful-fs@3.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 15.0.9

### Patch Changes

- Updated dependencies [955874422]
  - @pnpm/graceful-fs@2.1.0

## 15.0.8

### Patch Changes

- 029143cff: When resolving dependencies, prefer versions that are already used in the root of the project. This is important to minimize the number of packages that will be nested during hoisting [#6054](https://github.com/pnpm/pnpm/pull/6054).
- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0

## 15.0.7

### Patch Changes

- 74b535f19: Deduplicate direct dependencies.

  Let's say there are two projects in the workspace that dependend on `foo`. One project has `foo@1.0.0` in the dependencies while another one has `foo@^1.0.0` in the dependencies. In this case, `foo@1.0.0` should be installed to both projects as satisfies the version specs of both projects.

- 65563ae09: Return the lowest version when `pickLowestVersion` is `true` and the only versions in the metadata are prerelease versions.

## 15.0.6

### Patch Changes

- 1e6de89b6: Update ssri to v10.0.1.

## 15.0.5

### Patch Changes

- @pnpm/error@4.0.1

## 15.0.4

### Patch Changes

- 83ba90fb8: Throw an accurate error message when trying to install a package that has no versions, or all of its versions are unpublished [#5849](https://github.com/pnpm/pnpm/issues/5849).

## 15.0.3

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/core-loggers@8.0.3
  - @pnpm/resolver-base@9.1.5

## 15.0.2

### Patch Changes

- a9d59d8bc: Update dependencies.
- f3bfa2aae: `pnpm add` should prefer local projects from the workspace, even if they use prerelease versions.

## 15.0.1

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/core-loggers@8.0.2
  - @pnpm/resolver-base@9.1.4

## 15.0.0

### Major Changes

- 804de211e: GetCredentials function replaced with GetAuthHeader.

### Patch Changes

- Updated dependencies [804de211e]
  - @pnpm/fetching-types@4.0.0

## 14.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/core-loggers@8.0.1
  - @pnpm/resolver-base@9.1.3

## 14.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/core-loggers@8.0.0
  - @pnpm/resolve-workspace-range@4.0.0

## 13.1.11

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0

## 13.1.10

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0

## 13.1.9

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/core-loggers@7.0.8
  - @pnpm/resolver-base@9.1.2

## 13.1.8

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/core-loggers@7.0.7
  - @pnpm/resolver-base@9.1.1

## 13.1.7

### Patch Changes

- a3ccd27a3: `@types/ramda` should be a dev dependency.

## 13.1.6

### Patch Changes

- d7fc07cc7: Include `hasInstallScript` in the abbreviated metadata.

## 13.1.5

### Patch Changes

- 7fac3b446: Pick a version even if it was published after the given date (if there is no better match).

## 13.1.4

### Patch Changes

- 53506c7ae: Don't modify the manifest of the injected workspace project, when it has the same dependency in prod and peer dependencies.

## 13.1.3

### Patch Changes

- dbac0ca01: Update ssri to v9.

## 13.1.2

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/resolver-base@9.1.0

## 13.1.1

### Patch Changes

- 238a165a5: dependencies maintenance

## 13.1.0

### Minor Changes

- c90798461: When `publishConfig.directory` is set, only symlink it to other workspace projects if `publishConfig.linkDirectory` is set to `true`. Otherwise, only use it for publishing [#5115](https://github.com/pnpm/pnpm/issues/5115).

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/core-loggers@7.0.6
  - @pnpm/resolver-base@9.0.6

## 13.0.7

### Patch Changes

- eb2426cf8: When a project in a workspace has a `publishConfig.directory` set, dependent projects should install the project from that directory [#3901](https://github.com/pnpm/pnpm/issues/3901)

## 13.0.6

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/core-loggers@7.0.5
  - @pnpm/resolver-base@9.0.5

## 13.0.5

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/resolver-base@9.0.4

## 13.0.4

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/core-loggers@7.0.3
  - @pnpm/resolver-base@9.0.3

## 13.0.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2
  - @pnpm/resolver-base@9.0.2

## 13.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - @pnpm/resolver-base@9.0.1

## 13.0.1

### Patch Changes

- @pnpm/error@3.0.1

## 13.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/fetching-types@3.0.0
  - @pnpm/graceful-fs@2.0.0
  - @pnpm/resolve-workspace-range@3.0.0
  - @pnpm/resolver-base@9.0.0

## 12.1.8

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0

## 12.1.7

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/core-loggers@6.1.4
  - @pnpm/resolver-base@8.1.6

## 12.1.6

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - @pnpm/resolver-base@8.1.5

## 12.1.5

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - @pnpm/resolver-base@8.1.4

## 12.1.4

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - @pnpm/resolver-base@8.1.3

## 12.1.3

### Patch Changes

- 81ed15666: Always add a trailing slash to the registry URL [#4052](https://github.com/pnpm/pnpm/issues/4052).
- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/resolver-base@8.1.2

## 12.1.2

### Patch Changes

- 9f61bd81b: Downgrading `p-memoize` to v4.0.1. pnpm v6.22.0 started to print the next warning [#3989](https://github.com/pnpm/pnpm/issues/3989):

  ```
  (node:132923) TimeoutOverflowWarning: Infinity does not fit into a 32-bit signed integer.
  ```

## 12.1.1

### Patch Changes

- 108bd4a39: Injected directory resolutions should contain the relative path to the directory.
- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6
  - @pnpm/resolver-base@8.1.1

## 12.1.0

### Minor Changes

- 4ab87844a: Support the resolution of injected local dependencies.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/core-loggers@6.0.5

## 12.0.5

### Patch Changes

- 82caa0b56: It should be possible to alias scoped packages using the `workspace:` protocol. See https://github.com/pnpm/pnpm/issues/3883

## 12.0.4

### Patch Changes

- Updated dependencies [bab172385]
  - @pnpm/fetching-types@2.2.1

## 12.0.3

### Patch Changes

- eadf0e505: The metadata file should be requested in compressed state.
- Updated dependencies [eadf0e505]
  - @pnpm/fetching-types@2.2.0

## 12.0.2

### Patch Changes

- a4fed2798: Do not fail if a package has no shasum in the metadata.

  Fail if a package has broken shasum in the metadata.

## 12.0.1

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - @pnpm/resolver-base@8.0.4

## 12.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

## 11.1.4

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - @pnpm/resolver-base@8.0.3

## 11.1.3

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2
  - @pnpm/resolver-base@8.0.2

## 11.1.2

### Patch Changes

- ae36ac7d3: Fix: unhandled rejection in npm resolver when fetch fails
- bf322c702: Avoid conflicts in metadata, when a package name has upper case letters.

## 11.1.1

### Patch Changes

- Updated dependencies [a2aeeef88]
  - @pnpm/graceful-fs@1.0.0

## 11.1.0

### Minor Changes

- 85fb21a83: Add support for workspace:^ and workspace:~ aliases
- 05baaa6e7: Add new option: timeout.

### Patch Changes

- Updated dependencies [85fb21a83]
- Updated dependencies [05baaa6e7]
- Updated dependencies [97c64bae4]
  - @pnpm/resolve-workspace-range@2.1.0
  - @pnpm/fetching-types@2.1.0
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1
  - @pnpm/resolver-base@8.0.1

## 11.0.1

### Patch Changes

- 6f198457d: Update rename-overwrite.

## 11.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 83645c8ed: Update ssri.
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
  - @pnpm/core-loggers@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/fetching-types@2.0.0
  - @pnpm/resolve-workspace-range@2.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/types@7.0.0

## 10.2.2

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/core-loggers@5.0.3
  - @pnpm/resolver-base@7.1.1

## 10.2.1

### Patch Changes

- f47551a3c: Throw a meaningful error on malformed registry metadata.

## 10.2.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 10.1.0

### Minor Changes

- 284e95c5e: Skip workspace protocol specs that use relative path.
- 084614f55: Support aliases to workspace packages. For instance, `"foo": "workspace:bar@*"` will link bar from the repository but aliased to foo. Before publish, these specs are converted to regular aliased versions.

## 10.0.7

### Patch Changes

- 5ff6c28fa: Retry metadata download if the received JSON is broken.
- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0

## 10.0.6

### Patch Changes

- 39142e2ad: Update encode-registry to v3.

## 10.0.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/resolver-base@7.0.5

## 10.0.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/resolver-base@7.0.4

## 10.0.3

### Patch Changes

- d7b727795: Update p-memoize to v4.0.1.

## 10.0.2

### Patch Changes

- 3633f5e46: When no matching version is found, report the actually specified version spec in the error message (not the normalized one).

## 10.0.1

### Patch Changes

- 75a36deba: Report information about any used auth token, if an error happens during fetch.
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1

## 10.0.0

### Major Changes

- a1cdae3dc: Does not accept a `metaCache` option anymore. Caching happens internally, using `lru-cache`.

## 9.1.0

### Minor Changes

- 6d480dd7a: Report whether/what authorization header was used to make the request, when the request fails with an authorization issue.

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0

## 9.0.2

### Patch Changes

- 622c0b6f9: Always use the package name that is given at the root of the metadata object. Override any names that are specified in the version manifests. This fixes an issue with GitHub registry.
- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 9.0.1

### Patch Changes

- 379cdcaf8: When resolution from workspace fails, print the path to the project that has the unsatisfied dependency.

## 9.0.0

### Major Changes

- 71aeb9a38: Breaking changes to the API. fetchFromRegistry and getCredentials are passed in through arguments.

### Patch Changes

- Updated dependencies [71aeb9a38]
  - @pnpm/fetching-types@1.0.0

## 8.1.2

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/resolver-base@7.0.3
  - fetch-from-npm-registry@4.1.2

## 8.1.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/resolver-base@7.0.2
  - fetch-from-npm-registry@4.1.1

## 8.1.0

### Minor Changes

- 4cf7ef367: Reducing filesystem operations required to write the metadata file to the cache.

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [2ebb7af33]
  - fetch-from-npm-registry@4.1.0

## 8.0.1

### Patch Changes

- Updated dependencies [872f81ca1]
  - fetch-from-npm-registry@4.0.3

## 8.0.0

### Major Changes

- 5bc033c43: Reduce the number of directories in the store by keeping all the metadata json files in the same directory.

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/error@1.2.1
  - fetch-from-npm-registry@4.0.3
  - @pnpm/resolve-workspace-range@1.0.2
  - @pnpm/resolver-base@7.0.1

## 8.0.0-alpha.2

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 8.0.0-alpha.1

### Major Changes

- 5bc033c43: Reduce the number of directories in the store by keeping all the metadata json files in the same directory.

## 7.3.12-alpha.0

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
