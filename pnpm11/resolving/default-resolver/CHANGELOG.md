# @pnpm/default-resolver

## 1100.3.10

### Patch Changes

- Updated dependencies [bae694f]
- Updated dependencies [fa7004b]
- Updated dependencies [0ec878d]
- Updated dependencies [852d537]
  - @pnpm/resolving.npm-resolver@1102.1.0
  - @pnpm/resolving.resolver-base@1100.5.0
  - @pnpm/hooks.types@1100.1.0
  - @pnpm/engine.runtime.node-resolver@1101.1.9
  - @pnpm/engine.runtime.bun-resolver@1102.0.2
  - @pnpm/engine.runtime.deno-resolver@1102.0.2
  - @pnpm/error@1100.0.1
  - @pnpm/resolving.git-resolver@1100.1.7
  - @pnpm/resolving.local-resolver@1101.1.7
  - @pnpm/resolving.tarball-resolver@1100.1.5
  - @pnpm/network.auth-header@1101.1.3

## 1100.3.9

### Patch Changes

- Updated dependencies [29ab905]
- Updated dependencies [4ca9247]
  - @pnpm/resolving.npm-resolver@1102.0.1
  - @pnpm/engine.runtime.node-resolver@1101.1.8
  - @pnpm/engine.runtime.bun-resolver@1102.0.1
  - @pnpm/engine.runtime.deno-resolver@1102.0.1

## 1100.3.8

### Patch Changes

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

- Updated dependencies [61810aa]
- Updated dependencies [681b593]
- Updated dependencies [1310ab5]
- Updated dependencies [a31faa7]
  - @pnpm/resolving.npm-resolver@1102.0.0
  - @pnpm/fetching.types@1100.0.2
  - @pnpm/network.auth-header@1101.1.2
  - @pnpm/types@1101.3.2
  - @pnpm/engine.runtime.bun-resolver@1102.0.0
  - @pnpm/engine.runtime.deno-resolver@1102.0.0
  - @pnpm/engine.runtime.node-resolver@1101.1.7
  - @pnpm/resolving.git-resolver@1100.1.6
  - @pnpm/resolving.local-resolver@1101.1.6
  - @pnpm/resolving.tarball-resolver@1100.1.4
  - @pnpm/hooks.types@1100.0.12
  - @pnpm/resolving.resolver-base@1100.4.2

## 1100.3.7

### Patch Changes

- @pnpm/engine.runtime.node-resolver@1101.1.6
- @pnpm/resolving.npm-resolver@1101.5.2
- @pnpm/resolving.git-resolver@1100.1.5
- @pnpm/resolving.tarball-resolver@1100.1.3
- @pnpm/engine.runtime.bun-resolver@1101.1.7
- @pnpm/engine.runtime.deno-resolver@1101.1.7
- @pnpm/resolving.local-resolver@1101.1.5

## 1100.3.6

### Patch Changes

- Updated dependencies [bf1b731]
- Updated dependencies [3d50680]
  - @pnpm/types@1101.3.1
  - @pnpm/engine.runtime.node-resolver@1101.1.5
  - @pnpm/engine.runtime.bun-resolver@1101.1.6
  - @pnpm/engine.runtime.deno-resolver@1101.1.6
  - @pnpm/hooks.types@1100.0.11
  - @pnpm/network.auth-header@1101.1.1
  - @pnpm/resolving.local-resolver@1101.1.4
  - @pnpm/resolving.npm-resolver@1101.5.1
  - @pnpm/resolving.resolver-base@1100.4.1
  - @pnpm/resolving.git-resolver@1100.1.4
  - @pnpm/resolving.tarball-resolver@1100.1.3

## 1100.3.5

### Patch Changes

- 6d17b66: The lockfile verifier now checks that a registry entry pinning an explicit `tarball` URL points at the artifact the registry's own metadata lists for that `name@version`. Previously a tampered lockfile could pair a trusted `name@version` with an attacker-chosen tarball URL (and a matching integrity for those bytes), so the install fetched the attacker's bytes. A mismatch — or any entry that can't be confirmed against the registry — is rejected with `ERR_PNPM_TARBALL_URL_MISMATCH`. Non-registry resolutions (`file:`, git-hosted, etc.) and registry entries without an explicit tarball URL (the URL is reconstructed from name+version+registry, so it is inherently bound) are unaffected; non-standard registry tarball URLs (npm Enterprise, GitHub Packages) still pass because they match the metadata.

  This binding is unconditional — it runs regardless of `minimumReleaseAge`/`trustPolicy` and is not narrowed by their exclude lists, since it guards integrity rather than maturity/trust. It is **fail-closed**: an entry passes only when the registry metadata affirmatively lists the version with a matching tarball URL. If the metadata can't be fetched, doesn't list the version, or omits `dist.tarball`, the entry is rejected. As a result, an install that re-verifies a lockfile (any install whose lockfile content changed since the last verified run, where the verification cache no longer applies) now requires the configured registry to be reachable. `trustLockfile` is the opt-out for environments that treat the on-disk lockfile as already trusted.

  The `minimumReleaseAge`/`trustPolicy` verification also no longer applies to URL-keyed tarball dependencies (e.g. `https:` tarballs) that carry a semver `version` copied from their manifest — those are deliberate non-registry dependencies.

- Updated dependencies [5192edf]
- Updated dependencies [a017bf3]
- Updated dependencies [722b9cd]
- Updated dependencies [6d17b66]
  - @pnpm/network.auth-header@1101.1.0
  - @pnpm/types@1101.3.0
  - @pnpm/resolving.npm-resolver@1101.5.0
  - @pnpm/resolving.resolver-base@1100.4.0
  - @pnpm/engine.runtime.node-resolver@1101.1.4
  - @pnpm/resolving.git-resolver@1100.1.3
  - @pnpm/resolving.tarball-resolver@1100.1.2
  - @pnpm/engine.runtime.bun-resolver@1101.1.5
  - @pnpm/engine.runtime.deno-resolver@1101.1.5
  - @pnpm/hooks.types@1100.0.10
  - @pnpm/resolving.local-resolver@1101.1.3

## 1100.3.4

### Patch Changes

- Updated dependencies [6235428]
- Updated dependencies [1e9ab29]
  - @pnpm/resolving.npm-resolver@1101.4.0
  - @pnpm/engine.runtime.node-resolver@1101.1.3
  - @pnpm/resolving.git-resolver@1100.1.2
  - @pnpm/resolving.tarball-resolver@1100.1.1
  - @pnpm/engine.runtime.bun-resolver@1101.1.4
  - @pnpm/engine.runtime.deno-resolver@1101.1.4

## 1100.3.3

### Patch Changes

- Updated dependencies [a23956e]
- Updated dependencies [35d2355]
- Updated dependencies [0721d64]
  - @pnpm/network.auth-header@1101.0.0
  - @pnpm/types@1101.2.0
  - @pnpm/resolving.npm-resolver@1101.3.3
  - @pnpm/engine.runtime.node-resolver@1101.1.2
  - @pnpm/resolving.local-resolver@1101.1.2
  - @pnpm/engine.runtime.bun-resolver@1101.1.3
  - @pnpm/engine.runtime.deno-resolver@1101.1.3
  - @pnpm/hooks.types@1100.0.9
  - @pnpm/resolving.resolver-base@1100.3.1
  - @pnpm/resolving.git-resolver@1100.1.1
  - @pnpm/resolving.tarball-resolver@1100.1.1

## 1100.3.2

### Patch Changes

- Updated dependencies [212315d]
  - @pnpm/resolving.npm-resolver@1101.3.2
  - @pnpm/resolving.local-resolver@1101.1.1
  - @pnpm/engine.runtime.node-resolver@1101.1.1
  - @pnpm/engine.runtime.bun-resolver@1101.1.2
  - @pnpm/engine.runtime.deno-resolver@1101.1.2

## 1100.3.1

### Patch Changes

- @pnpm/resolving.npm-resolver@1101.3.1
- @pnpm/engine.runtime.bun-resolver@1101.1.1
- @pnpm/engine.runtime.deno-resolver@1101.1.1

## 1100.3.0

### Minor Changes

- 1627943: `pnpm outdated` and `pnpm update --interactive` now report Node.js, Deno, and Bun runtimes installed as project dependencies (`runtime:` specifiers). Previously these were silently skipped because the npm specifier parser did not understand the `runtime:` protocol, so runtime versions never appeared in the outdated table or the interactive update picker.

  Internally, the outdated check is now resolver-driven: `@pnpm/resolving.resolver-base` defines a `ResolveLatestFunction` shape (with `LatestQuery` input — `{ wantedDependency, compatible? }` — and `LatestInfo` result — `{ latestManifest? }`), and every protocol resolver (npm, jsr, named-registry, git, tarball, local, node/bun/deno runtimes) exports its own `resolveLatest*` function alongside its `resolve*`. `@pnpm/resolving.default-resolver` composes them into a single dispatcher, exposed through `@pnpm/installing.client` as `createResolver(...).resolveLatest`.

  Each resolver decides whether it owns the dep and what "latest" means for its protocol; the outdated command derives `current` / `wanted` display values from the lockfile snapshot (`pkgSnapshot.version` for semver protocols, raw ref for URL-shaped ones) and uses raw ref equality for the "lockfile changed" check, so protocol knowledge stays inside each resolver instead of the command.

### Patch Changes

- Updated dependencies [3a54205]
- Updated dependencies [1627943]
- Updated dependencies [64afc92]
  - @pnpm/resolving.npm-resolver@1101.3.0
  - @pnpm/resolving.resolver-base@1100.3.0
  - @pnpm/resolving.git-resolver@1100.1.0
  - @pnpm/resolving.tarball-resolver@1100.1.0
  - @pnpm/resolving.local-resolver@1101.1.0
  - @pnpm/engine.runtime.node-resolver@1101.1.0
  - @pnpm/engine.runtime.bun-resolver@1101.1.0
  - @pnpm/engine.runtime.deno-resolver@1101.1.0
  - @pnpm/types@1101.1.1
  - @pnpm/hooks.types@1100.0.8
  - @pnpm/network.auth-header@1100.0.3

## 1100.2.0

### Minor Changes

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

- Updated dependencies [963861c]
- Updated dependencies [4195766]
- Updated dependencies [31538bf]
  - @pnpm/resolving.npm-resolver@1101.2.0
  - @pnpm/resolving.resolver-base@1100.2.0
  - @pnpm/engine.runtime.bun-resolver@1101.0.7
  - @pnpm/engine.runtime.deno-resolver@1101.0.7
  - @pnpm/engine.runtime.node-resolver@1101.0.9
  - @pnpm/hooks.types@1100.0.7
  - @pnpm/resolving.git-resolver@1100.0.8
  - @pnpm/resolving.local-resolver@1101.0.2
  - @pnpm/resolving.tarball-resolver@1100.0.6

## 1100.1.2

### Patch Changes

- Updated dependencies [50b33c1]
- Updated dependencies [e526f89]
- Updated dependencies [c2c2890]
  - @pnpm/resolving.npm-resolver@1101.1.1
  - @pnpm/engine.runtime.bun-resolver@1101.0.6
  - @pnpm/engine.runtime.deno-resolver@1101.0.6
  - @pnpm/engine.runtime.node-resolver@1101.0.8
  - @pnpm/resolving.git-resolver@1100.0.7
  - @pnpm/resolving.tarball-resolver@1100.0.5
  - @pnpm/resolving.local-resolver@1101.0.1

## 1100.1.1

### Patch Changes

- 3ab403a: Fixed `pnpm add <alias>:@scope/pkg` for [named registries](https://github.com/pnpm/pnpm/pull/11324). The local resolver was claiming any specifier containing `/` as a local directory, so `pnpm add bit:@teambit/bit` (with `bit` configured under `namedRegistries`) installed a bogus link to `bit:@teambit/bit/` instead of resolving from the configured registry. The local resolver now runs after the named-registry resolver in the resolution chain.
- Updated dependencies [3ab403a]
  - @pnpm/resolving.local-resolver@1101.0.0

## 1100.1.0

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
  - @pnpm/resolving.npm-resolver@1101.1.0
  - @pnpm/types@1101.1.0
  - @pnpm/engine.runtime.node-resolver@1101.0.7
  - @pnpm/resolving.git-resolver@1100.0.6
  - @pnpm/resolving.tarball-resolver@1100.0.5
  - @pnpm/engine.runtime.bun-resolver@1101.0.5
  - @pnpm/engine.runtime.deno-resolver@1101.0.5
  - @pnpm/hooks.types@1100.0.6
  - @pnpm/resolving.local-resolver@1100.0.6
  - @pnpm/resolving.resolver-base@1100.1.3

## 1100.0.11

### Patch Changes

- Updated dependencies [a57f7bd]
- Updated dependencies [15e9e35]
  - @pnpm/resolving.git-resolver@1100.0.5
  - @pnpm/resolving.npm-resolver@1101.0.3
  - @pnpm/engine.runtime.node-resolver@1101.0.6
  - @pnpm/engine.runtime.bun-resolver@1101.0.4
  - @pnpm/engine.runtime.deno-resolver@1101.0.4

## 1100.0.10

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/resolving.git-resolver@1100.0.4
  - @pnpm/resolving.resolver-base@1100.1.2
  - @pnpm/hooks.types@1100.0.5
  - @pnpm/engine.runtime.bun-resolver@1101.0.3
  - @pnpm/engine.runtime.deno-resolver@1101.0.3
  - @pnpm/engine.runtime.node-resolver@1101.0.5
  - @pnpm/resolving.local-resolver@1100.0.5
  - @pnpm/resolving.npm-resolver@1101.0.2
  - @pnpm/resolving.tarball-resolver@1100.0.4

## 1100.0.9

### Patch Changes

- @pnpm/engine.runtime.node-resolver@1101.0.4

## 1100.0.8

### Patch Changes

- @pnpm/engine.runtime.node-resolver@1101.0.3

## 1100.0.7

### Patch Changes

- 184ce26: Fix the package name in README.md.
- Updated dependencies [184ce26]
  - @pnpm/resolving.tarball-resolver@1100.0.3
  - @pnpm/resolving.local-resolver@1100.0.4
  - @pnpm/resolving.resolver-base@1100.1.1
  - @pnpm/resolving.git-resolver@1100.0.3
  - @pnpm/resolving.npm-resolver@1101.0.1
  - @pnpm/fetching.types@1100.0.1
  - @pnpm/engine.runtime.bun-resolver@1101.0.2
  - @pnpm/engine.runtime.deno-resolver@1101.0.2
  - @pnpm/engine.runtime.node-resolver@1101.0.2
  - @pnpm/hooks.types@1100.0.4

## 1100.0.6

### Patch Changes

- @pnpm/engine.runtime.node-resolver@1101.0.1

## 1100.0.5

### Patch Changes

- @pnpm/engine.runtime.bun-resolver@1101.0.1
- @pnpm/engine.runtime.deno-resolver@1101.0.1

## 1100.0.4

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/engine.runtime.node-resolver@1101.0.0
  - @pnpm/engine.runtime.bun-resolver@1101.0.0
  - @pnpm/engine.runtime.deno-resolver@1101.0.0
  - @pnpm/hooks.types@1100.0.3
  - @pnpm/resolving.npm-resolver@1101.0.0

## 1100.0.3

### Patch Changes

- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/resolving.resolver-base@1100.1.0
  - @pnpm/resolving.npm-resolver@1100.1.0
  - @pnpm/engine.runtime.node-resolver@1100.0.3
  - @pnpm/engine.runtime.bun-resolver@1100.0.2
  - @pnpm/engine.runtime.deno-resolver@1100.0.2
  - @pnpm/hooks.types@1100.0.2
  - @pnpm/resolving.git-resolver@1100.0.2
  - @pnpm/resolving.local-resolver@1100.0.3
  - @pnpm/resolving.tarball-resolver@1100.0.2

## 1100.0.2

### Patch Changes

- @pnpm/engine.runtime.node-resolver@1100.0.2
- @pnpm/resolving.local-resolver@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/engine.runtime.bun-resolver@1100.0.1
  - @pnpm/engine.runtime.deno-resolver@1100.0.1
  - @pnpm/engine.runtime.node-resolver@1100.0.1
  - @pnpm/hooks.types@1100.0.1
  - @pnpm/resolving.local-resolver@1100.0.1
  - @pnpm/resolving.npm-resolver@1100.0.1
  - @pnpm/resolving.resolver-base@1100.0.1
  - @pnpm/resolving.git-resolver@1100.0.1
  - @pnpm/resolving.tarball-resolver@1100.0.1

## 1003.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- ec7c5d7: Support plain `http://` and `https://` URLs ending with `.git` as git repository dependencies.

  Previously, URLs like `https://gitea.example.org/user/repo.git#commit` were not recognized as git repositories because they lacked the `git+` prefix (e.g., `git+https://`). This caused issues when installing dependencies from self-hosted git servers like Gitea or Forgejo that don't provide tarball downloads.

  Changes:

  - The git resolver now runs before the tarball resolver, ensuring git URLs are handled by the correct resolver
  - The git resolver now recognizes plain `http://` and `https://` URLs ending in `.git` as git repositories
  - Removed the `isRepository` check from the tarball resolver since it's no longer needed with the new resolver order

  Fixes #10468

- 96704a1: Renamed `rawConfig` to `authConfig` on the `Config` interface. This field now only contains auth/registry data from `.npmrc` files. Non-auth settings are no longer written to it.

  Added `nodeDownloadMirrors` setting to configure custom Node.js download mirrors in `pnpm-workspace.yaml`:

  ```yaml
  nodeDownloadMirrors:
    release: https://my-mirror.example.com/download/release/
    nightly: https://my-mirror.example.com/download/nightly/
  ```

  Replaced `rawConfig: object` with `userAgent?: string` in lifecycle hook options. Removed unused `rawConfig` from fetcher and prepare-package options.

  Removed support for the npm `init-module` setting. Custom init scripts via `.pnpm-init.js` are no longer executed by `pnpm init`.

- 38b8e35: Support for custom resolvers and fetchers.

### Patch Changes

- Updated dependencies [facdd71]
- Updated dependencies [9b0a460]
- Updated dependencies [a297ebc]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [831f574]
- Updated dependencies [0e9c559]
- Updated dependencies [19f36cf]
- Updated dependencies [491a84f]
- Updated dependencies [01760da]
- Updated dependencies [ec7c5d7]
- Updated dependencies [61cad0c]
- Updated dependencies [19f36cf]
- Updated dependencies [c5fbdde]
- Updated dependencies [23eb4a6]
- Updated dependencies [143ca78]
- Updated dependencies [6f361aa]
- Updated dependencies [0625e20]
- Updated dependencies [938ea1f]
- Updated dependencies [9065f49]
- Updated dependencies [2cb0657]
- Updated dependencies [bb8baa7]
- Updated dependencies [7d2fd48]
- Updated dependencies [144ce0e]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [6c480a4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [499ef22]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [ba70035]
- Updated dependencies [3585d9a]
- Updated dependencies [38b8e35]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [9d3f00b]
- Updated dependencies [e0f0a7d]
- Updated dependencies [6557dc0]
- Updated dependencies [efb48dc]
  - @pnpm/resolving.resolver-base@1006.0.0
  - @pnpm/resolving.npm-resolver@1005.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/resolving.tarball-resolver@1003.0.0
  - @pnpm/resolving.local-resolver@1003.0.0
  - @pnpm/engine.runtime.deno-resolver@1003.0.0
  - @pnpm/fetching.types@1001.0.0
  - @pnpm/engine.runtime.bun-resolver@1003.0.0
  - @pnpm/resolving.git-resolver@1002.0.0
  - @pnpm/engine.runtime.node-resolver@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/hooks.types@1002.0.0

## 1002.2.12

### Patch Changes

- Updated dependencies [6c3dcb8]
  - @pnpm/npm-resolver@1004.4.1
  - @pnpm/resolving.bun-resolver@1002.0.1
  - @pnpm/resolving.deno-resolver@1002.0.1

## 1002.2.11

### Patch Changes

- Updated dependencies [7c1382f]
  - @pnpm/resolver-base@1005.1.0
  - @pnpm/npm-resolver@1004.4.0
  - @pnpm/resolving.bun-resolver@1002.0.0
  - @pnpm/resolving.deno-resolver@1002.0.0
  - @pnpm/node.resolver@1001.0.5
  - @pnpm/local-resolver@1002.1.4
  - @pnpm/git-resolver@1001.1.5
  - @pnpm/tarball-resolver@1002.1.4

## 1002.2.10

### Patch Changes

- @pnpm/node.resolver@1001.0.4
- @pnpm/resolving.bun-resolver@1001.0.1
- @pnpm/resolving.deno-resolver@1001.0.1

## 1002.2.9

### Patch Changes

- @pnpm/resolving.bun-resolver@1001.0.0
- @pnpm/resolving.deno-resolver@1001.0.0

## 1002.2.8

### Patch Changes

- Updated dependencies [fb4da0c]
  - @pnpm/npm-resolver@1004.3.0
  - @pnpm/resolving.bun-resolver@1000.0.7
  - @pnpm/resolving.deno-resolver@1000.0.7
  - @pnpm/node.resolver@1001.0.3
  - @pnpm/local-resolver@1002.1.3

## 1002.2.7

### Patch Changes

- Updated dependencies [baf8bf6]
- Updated dependencies [702ddb9]
  - @pnpm/npm-resolver@1004.2.3
  - @pnpm/resolving.bun-resolver@1000.0.6
  - @pnpm/resolving.deno-resolver@1000.0.6

## 1002.2.6

### Patch Changes

- Updated dependencies [121b44e]
- Updated dependencies [02f8b69]
  - @pnpm/npm-resolver@1004.2.2
  - @pnpm/resolving.bun-resolver@1000.0.5
  - @pnpm/resolving.deno-resolver@1000.0.5

## 1002.2.5

### Patch Changes

- @pnpm/node.resolver@1001.0.2
- @pnpm/error@1000.0.5
- @pnpm/resolving.bun-resolver@1000.0.4
- @pnpm/resolving.deno-resolver@1000.0.4
- @pnpm/npm-resolver@1004.2.1
- @pnpm/local-resolver@1002.1.2

## 1002.2.4

### Patch Changes

- Updated dependencies [38e2599]
  - @pnpm/npm-resolver@1004.2.0
  - @pnpm/resolving.bun-resolver@1000.0.3
  - @pnpm/resolving.deno-resolver@1000.0.3
  - @pnpm/node.resolver@1001.0.1
  - @pnpm/local-resolver@1002.1.1
  - @pnpm/resolver-base@1005.0.1
  - @pnpm/git-resolver@1001.1.4
  - @pnpm/tarball-resolver@1002.1.3

## 1002.2.3

### Patch Changes

- @pnpm/resolving.bun-resolver@1000.0.2
- @pnpm/resolving.deno-resolver@1000.0.2

## 1002.2.2

### Patch Changes

- @pnpm/node.resolver@1001.0.0
- @pnpm/git-resolver@1001.1.3
- @pnpm/npm-resolver@1004.1.3
- @pnpm/tarball-resolver@1002.1.2

## 1002.2.1

### Patch Changes

- Updated dependencies [2b0d35f]
  - @pnpm/resolving.deno-resolver@1000.0.1
  - @pnpm/resolving.bun-resolver@1000.0.1

## 1002.2.0

### Minor Changes

- d1edf73: Add support for installing deno runtime.
- 86b33e9: Added support for installing Bun runtime.

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [5dedada]
- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/resolving.deno-resolver@1000.0.0
  - @pnpm/node.resolver@1001.0.0
  - @pnpm/resolving.bun-resolver@1000.0.0
  - @pnpm/resolver-base@1005.0.0
  - @pnpm/local-resolver@1002.1.0
  - @pnpm/error@1000.0.4
  - @pnpm/npm-resolver@1004.1.3
  - @pnpm/git-resolver@1001.1.2
  - @pnpm/tarball-resolver@1002.1.2

## 1002.1.2

### Patch Changes

- Updated dependencies [1ba2e15]
- Updated dependencies [1a07b8f]
  - @pnpm/fetching-types@1000.2.0
  - @pnpm/resolver-base@1004.1.0
  - @pnpm/node.resolver@1000.1.0
  - @pnpm/local-resolver@1002.0.2
  - @pnpm/npm-resolver@1004.1.2
  - @pnpm/tarball-resolver@1002.1.1
  - @pnpm/git-resolver@1001.1.1
  - @pnpm/error@1000.0.3

## 1002.1.1

### Patch Changes

- @pnpm/local-resolver@1002.0.1
- @pnpm/npm-resolver@1004.1.1

## 1002.1.0

### Minor Changes

- 2721291: Create different resolver result types which provide more information.

### Patch Changes

- Updated dependencies [2721291]
- Updated dependencies [6acf819]
  - @pnpm/tarball-resolver@1002.1.0
  - @pnpm/local-resolver@1002.0.0
  - @pnpm/resolver-base@1004.0.0
  - @pnpm/git-resolver@1001.1.0
  - @pnpm/npm-resolver@1004.1.0

## 1002.0.2

### Patch Changes

- Updated dependencies [c307634]
- Updated dependencies [5055399]
  - @pnpm/tarball-resolver@1002.0.2
  - @pnpm/git-resolver@1001.0.2

## 1002.0.1

### Patch Changes

- Updated dependencies [09cf46f]
- Updated dependencies [6b6ccf9]
  - @pnpm/local-resolver@1001.0.1
  - @pnpm/npm-resolver@1004.0.1
  - @pnpm/git-resolver@1001.0.1
  - @pnpm/tarball-resolver@1002.0.1
  - @pnpm/resolver-base@1003.0.1

## 1002.0.0

### Major Changes

- 8a9f3a4: `pref` renamed to `bareSpecifier`.

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
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
  - @pnpm/tarball-resolver@1002.0.0
  - @pnpm/local-resolver@1001.0.0
  - @pnpm/resolver-base@1003.0.0
  - @pnpm/git-resolver@1001.0.0
  - @pnpm/npm-resolver@1004.0.0

## 1001.0.13

### Patch Changes

- Updated dependencies [81f441c]
  - @pnpm/resolver-base@1002.0.0
  - @pnpm/npm-resolver@1003.0.0
  - @pnpm/git-resolver@1000.0.11
  - @pnpm/local-resolver@1000.0.12
  - @pnpm/tarball-resolver@1001.0.8

## 1001.0.12

### Patch Changes

- Updated dependencies [72cff38]
  - @pnpm/resolver-base@1001.0.0
  - @pnpm/npm-resolver@1002.0.0
  - @pnpm/local-resolver@1000.0.11
  - @pnpm/git-resolver@1000.0.10
  - @pnpm/tarball-resolver@1001.0.7

## 1001.0.11

### Patch Changes

- @pnpm/local-resolver@1000.0.10
- @pnpm/npm-resolver@1001.0.1
- @pnpm/resolver-base@1000.2.1
- @pnpm/git-resolver@1000.0.9
- @pnpm/tarball-resolver@1001.0.6

## 1001.0.10

### Patch Changes

- Updated dependencies [3d52365]
  - @pnpm/resolver-base@1000.2.0
  - @pnpm/npm-resolver@1001.0.0
  - @pnpm/git-resolver@1000.0.8
  - @pnpm/local-resolver@1000.0.9
  - @pnpm/tarball-resolver@1001.0.5

## 1001.0.9

### Patch Changes

- @pnpm/local-resolver@1000.0.8
- @pnpm/npm-resolver@1000.1.7

## 1001.0.8

### Patch Changes

- Updated dependencies [8371664]
  - @pnpm/npm-resolver@1000.1.6

## 1001.0.7

### Patch Changes

- @pnpm/local-resolver@1000.0.7
- @pnpm/npm-resolver@1000.1.5
- @pnpm/resolver-base@1000.1.4
- @pnpm/git-resolver@1000.0.7
- @pnpm/tarball-resolver@1001.0.4

## 1001.0.6

### Patch Changes

- @pnpm/local-resolver@1000.0.6
- @pnpm/npm-resolver@1000.1.4
- @pnpm/resolver-base@1000.1.3
- @pnpm/git-resolver@1000.0.6
- @pnpm/tarball-resolver@1001.0.3

## 1001.0.5

### Patch Changes

- Updated dependencies [d6a4ff1]
  - @pnpm/git-resolver@1000.0.5
  - @pnpm/local-resolver@1000.0.5

## 1001.0.4

### Patch Changes

- @pnpm/error@1000.0.2
- @pnpm/npm-resolver@1000.1.3
- @pnpm/local-resolver@1000.0.4
- @pnpm/resolver-base@1000.1.2
- @pnpm/git-resolver@1000.0.4
- @pnpm/tarball-resolver@1001.0.2

## 1001.0.3

### Patch Changes

- @pnpm/local-resolver@1000.0.3

## 1001.0.2

### Patch Changes

- @pnpm/local-resolver@1000.0.2
- @pnpm/npm-resolver@1000.1.2
- @pnpm/resolver-base@1000.1.1
- @pnpm/git-resolver@1000.0.3
- @pnpm/tarball-resolver@1001.0.1

## 1001.0.1

### Patch Changes

- Updated dependencies [b100962]
  - @pnpm/git-resolver@1000.0.2
  - @pnpm/npm-resolver@1000.1.1
  - @pnpm/tarball-resolver@1001.0.0

## 1001.0.0

### Major Changes

- b0f3c71: Dependencies specified via a URL are now recorded in the lockfile using their final resolved URL. Thus, if the original URL redirects, the final redirect target will be saved in the lockfile [#8833](https://github.com/pnpm/pnpm/issues/8833).

### Patch Changes

- Updated dependencies [6483b64]
- Updated dependencies [b0f3c71]
- Updated dependencies [b0f3c71]
  - @pnpm/resolver-base@1000.1.0
  - @pnpm/npm-resolver@1000.1.0
  - @pnpm/tarball-resolver@1001.0.0
  - @pnpm/fetching-types@1000.1.0
  - @pnpm/error@1000.0.1
  - @pnpm/git-resolver@1000.0.1
  - @pnpm/local-resolver@1000.0.1

## 20.0.10

### Patch Changes

- Updated dependencies [3be45b7]
- Updated dependencies [501c152]
  - @pnpm/tarball-resolver@9.0.8
  - @pnpm/npm-resolver@22.0.0
  - @pnpm/error@6.0.3
  - @pnpm/local-resolver@12.0.10

## 20.0.9

### Patch Changes

- Updated dependencies [222d10a]
  - @pnpm/npm-resolver@21.1.1

## 20.0.8

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/npm-resolver@21.1.0
  - @pnpm/error@6.0.2
  - @pnpm/local-resolver@12.0.9

## 20.0.7

### Patch Changes

- @pnpm/local-resolver@12.0.8
- @pnpm/npm-resolver@21.0.5
- @pnpm/resolver-base@13.0.4
- @pnpm/git-resolver@9.0.8
- @pnpm/tarball-resolver@9.0.7

## 20.0.6

### Patch Changes

- @pnpm/local-resolver@12.0.7
- @pnpm/npm-resolver@21.0.4
- @pnpm/resolver-base@13.0.3
- @pnpm/git-resolver@9.0.7
- @pnpm/tarball-resolver@9.0.6

## 20.0.5

### Patch Changes

- @pnpm/local-resolver@12.0.6
- @pnpm/npm-resolver@21.0.3
- @pnpm/resolver-base@13.0.2
- @pnpm/git-resolver@9.0.6
- @pnpm/tarball-resolver@9.0.5

## 20.0.4

### Patch Changes

- @pnpm/local-resolver@12.0.5
- @pnpm/npm-resolver@21.0.2
- @pnpm/resolver-base@13.0.1
- @pnpm/git-resolver@9.0.5
- @pnpm/tarball-resolver@9.0.4

## 20.0.3

### Patch Changes

- Updated dependencies [afe520d]
  - @pnpm/npm-resolver@21.0.1

## 20.0.2

### Patch Changes

- Updated dependencies [dd00eeb]
  - @pnpm/resolver-base@13.0.0
  - @pnpm/npm-resolver@21.0.0
  - @pnpm/git-resolver@9.0.4
  - @pnpm/local-resolver@12.0.4
  - @pnpm/tarball-resolver@9.0.3

## 20.0.1

### Patch Changes

- @pnpm/local-resolver@12.0.3
- @pnpm/npm-resolver@20.0.1
- @pnpm/resolver-base@12.0.2
- @pnpm/git-resolver@9.0.3
- @pnpm/tarball-resolver@9.0.2

## 20.0.0

### Major Changes

- 0c08e1c: Breaking change.

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/npm-resolver@20.0.0

## 19.0.5

### Patch Changes

- @pnpm/local-resolver@12.0.2
- @pnpm/npm-resolver@19.0.4
- @pnpm/resolver-base@12.0.1
- @pnpm/git-resolver@9.0.2
- @pnpm/tarball-resolver@9.0.1

## 19.0.4

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/local-resolver@12.0.1
  - @pnpm/npm-resolver@19.0.3

## 19.0.3

### Patch Changes

- Updated dependencies [43b6bb7]
  - @pnpm/npm-resolver@19.0.2

## 19.0.2

### Patch Changes

- Updated dependencies [cb0f459]
  - @pnpm/npm-resolver@19.0.1

## 19.0.1

### Patch Changes

- Updated dependencies [c969f37]
  - @pnpm/git-resolver@9.0.1

## 19.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [43cdd87]
- Updated dependencies [985381c]
- Updated dependencies [d381a60]
- Updated dependencies [b13d2dc]
  - @pnpm/error@6.0.0
  - @pnpm/npm-resolver@19.0.0
  - @pnpm/tarball-resolver@9.0.0
  - @pnpm/local-resolver@12.0.0
  - @pnpm/resolver-base@12.0.0
  - @pnpm/fetching-types@6.0.0
  - @pnpm/git-resolver@9.0.0

## 18.0.22

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0
  - @pnpm/npm-resolver@18.1.0
  - @pnpm/git-resolver@8.0.12
  - @pnpm/local-resolver@11.0.4
  - @pnpm/tarball-resolver@8.0.8

## 18.0.21

### Patch Changes

- Updated dependencies [33313d2fd]
  - @pnpm/npm-resolver@18.0.2
  - @pnpm/local-resolver@11.0.3
  - @pnpm/resolver-base@11.0.2
  - @pnpm/git-resolver@8.0.11
  - @pnpm/tarball-resolver@8.0.7

## 18.0.20

### Patch Changes

- @pnpm/local-resolver@11.0.2
- @pnpm/npm-resolver@18.0.1
- @pnpm/resolver-base@11.0.1
- @pnpm/git-resolver@8.0.10
- @pnpm/tarball-resolver@8.0.6

## 18.0.19

### Patch Changes

- Updated dependencies [cd4fcfff0]
  - @pnpm/npm-resolver@18.0.0

## 18.0.18

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/resolver-base@11.0.0
  - @pnpm/npm-resolver@17.0.0
  - @pnpm/git-resolver@8.0.9
  - @pnpm/local-resolver@11.0.1
  - @pnpm/tarball-resolver@8.0.5

## 18.0.17

### Patch Changes

- Updated dependencies [5b7ed47d8]
- Updated dependencies [5b7ed47d8]
  - @pnpm/local-resolver@11.0.0
  - @pnpm/npm-resolver@16.0.13
  - @pnpm/resolver-base@10.0.4
  - @pnpm/git-resolver@8.0.8
  - @pnpm/tarball-resolver@8.0.4

## 18.0.16

### Patch Changes

- Updated dependencies [01bc58e2c]
- Updated dependencies [ff55119a8]
  - @pnpm/local-resolver@10.0.9
  - @pnpm/npm-resolver@16.0.12

## 18.0.15

### Patch Changes

- @pnpm/local-resolver@10.0.8
- @pnpm/npm-resolver@16.0.11
- @pnpm/resolver-base@10.0.3
- @pnpm/git-resolver@8.0.7
- @pnpm/tarball-resolver@8.0.3

## 18.0.14

### Patch Changes

- @pnpm/local-resolver@10.0.7
- @pnpm/npm-resolver@16.0.10

## 18.0.13

### Patch Changes

- Updated dependencies [41c2b65cf]
  - @pnpm/npm-resolver@16.0.9
  - @pnpm/local-resolver@10.0.6

## 18.0.12

### Patch Changes

- Updated dependencies [22bbe9255]
  - @pnpm/git-resolver@8.0.6
  - @pnpm/npm-resolver@16.0.8

## 18.0.11

### Patch Changes

- Updated dependencies [de9b6c20d]
  - @pnpm/git-resolver@8.0.5

## 18.0.10

### Patch Changes

- Updated dependencies [6fe0b60e6]
- Updated dependencies [e958707b2]
  - @pnpm/git-resolver@8.0.4
  - @pnpm/npm-resolver@16.0.8
  - @pnpm/local-resolver@10.0.5
  - @pnpm/resolver-base@10.0.2
  - @pnpm/tarball-resolver@8.0.2

## 18.0.9

### Patch Changes

- @pnpm/local-resolver@10.0.4

## 18.0.8

### Patch Changes

- @pnpm/error@5.0.2
- @pnpm/local-resolver@10.0.3
- @pnpm/npm-resolver@16.0.7

## 18.0.7

### Patch Changes

- Updated dependencies [d55b41a8b]
  - @pnpm/local-resolver@10.0.2
  - @pnpm/npm-resolver@16.0.6

## 18.0.6

### Patch Changes

- Updated dependencies [e6052260c]
  - @pnpm/npm-resolver@16.0.5
  - @pnpm/local-resolver@10.0.1
  - @pnpm/resolver-base@10.0.1
  - @pnpm/error@5.0.1
  - @pnpm/git-resolver@8.0.3
  - @pnpm/tarball-resolver@8.0.1

## 18.0.5

### Patch Changes

- Updated dependencies [edb3072a9]
  - @pnpm/npm-resolver@16.0.4

## 18.0.4

### Patch Changes

- @pnpm/git-resolver@8.0.2
- @pnpm/npm-resolver@16.0.3

## 18.0.3

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/git-resolver@8.0.1
  - @pnpm/npm-resolver@16.0.3

## 18.0.2

### Patch Changes

- Updated dependencies [ef6c22e12]
  - @pnpm/npm-resolver@16.0.2

## 18.0.1

### Patch Changes

- Updated dependencies [642f8c1d0]
  - @pnpm/npm-resolver@16.0.1

## 18.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [28796377c]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [f835994ea]
- Updated dependencies [9d026b7cb]
  - @pnpm/git-resolver@8.0.0
  - @pnpm/tarball-resolver@8.0.0
  - @pnpm/local-resolver@10.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/fetching-types@5.0.0
  - @pnpm/npm-resolver@16.0.0
  - @pnpm/error@5.0.0

## 17.0.11

### Patch Changes

- @pnpm/local-resolver@9.0.9
- @pnpm/npm-resolver@15.0.9

## 17.0.10

### Patch Changes

- @pnpm/git-resolver@7.0.7
- @pnpm/npm-resolver@15.0.8

## 17.0.9

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0
  - @pnpm/npm-resolver@15.0.8
  - @pnpm/git-resolver@7.0.6
  - @pnpm/local-resolver@9.0.8
  - @pnpm/tarball-resolver@7.0.4

## 17.0.8

### Patch Changes

- Updated dependencies [74b535f19]
- Updated dependencies [65563ae09]
  - @pnpm/npm-resolver@15.0.7

## 17.0.7

### Patch Changes

- Updated dependencies [1e6de89b6]
  - @pnpm/local-resolver@9.0.7
  - @pnpm/npm-resolver@15.0.6

## 17.0.6

### Patch Changes

- @pnpm/error@4.0.1
- @pnpm/local-resolver@9.0.6
- @pnpm/npm-resolver@15.0.5

## 17.0.5

### Patch Changes

- Updated dependencies [83ba90fb8]
  - @pnpm/npm-resolver@15.0.4

## 17.0.4

### Patch Changes

- @pnpm/local-resolver@9.0.5
- @pnpm/npm-resolver@15.0.3
- @pnpm/resolver-base@9.1.5
- @pnpm/git-resolver@7.0.5
- @pnpm/tarball-resolver@7.0.3

## 17.0.3

### Patch Changes

- Updated dependencies [a9d59d8bc]
- Updated dependencies [f3bfa2aae]
  - @pnpm/local-resolver@9.0.4
  - @pnpm/npm-resolver@15.0.2
  - @pnpm/git-resolver@7.0.4

## 17.0.2

### Patch Changes

- @pnpm/local-resolver@9.0.3

## 17.0.1

### Patch Changes

- @pnpm/local-resolver@9.0.2
- @pnpm/npm-resolver@15.0.1
- @pnpm/resolver-base@9.1.4
- @pnpm/git-resolver@7.0.3
- @pnpm/tarball-resolver@7.0.2

## 17.0.0

### Major Changes

- 804de211e: GetCredentials function replaced with GetAuthHeader.

### Patch Changes

- Updated dependencies [804de211e]
  - @pnpm/fetching-types@4.0.0
  - @pnpm/npm-resolver@15.0.0
  - @pnpm/git-resolver@7.0.2

## 16.0.1

### Patch Changes

- @pnpm/local-resolver@9.0.1
- @pnpm/npm-resolver@14.0.1
- @pnpm/resolver-base@9.1.3
- @pnpm/git-resolver@7.0.1
- @pnpm/tarball-resolver@7.0.1

## 16.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/git-resolver@7.0.0
  - @pnpm/local-resolver@9.0.0
  - @pnpm/npm-resolver@14.0.0
  - @pnpm/tarball-resolver@7.0.0

## 15.0.24

### Patch Changes

- @pnpm/local-resolver@8.0.15

## 15.0.23

### Patch Changes

- @pnpm/npm-resolver@13.1.11
- @pnpm/git-resolver@6.1.7

## 15.0.22

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/local-resolver@8.0.14
  - @pnpm/npm-resolver@13.1.10

## 15.0.21

### Patch Changes

- @pnpm/local-resolver@8.0.13
- @pnpm/npm-resolver@13.1.9
- @pnpm/resolver-base@9.1.2
- @pnpm/git-resolver@6.1.6
- @pnpm/tarball-resolver@6.0.9

## 15.0.20

### Patch Changes

- @pnpm/local-resolver@8.0.12
- @pnpm/npm-resolver@13.1.8
- @pnpm/resolver-base@9.1.1
- @pnpm/git-resolver@6.1.5
- @pnpm/tarball-resolver@6.0.8

## 15.0.19

### Patch Changes

- Updated dependencies [a3ccd27a3]
  - @pnpm/npm-resolver@13.1.7

## 15.0.18

### Patch Changes

- Updated dependencies [d7fc07cc7]
  - @pnpm/npm-resolver@13.1.6

## 15.0.17

### Patch Changes

- Updated dependencies [7fac3b446]
  - @pnpm/npm-resolver@13.1.5

## 15.0.16

### Patch Changes

- Updated dependencies [53506c7ae]
  - @pnpm/npm-resolver@13.1.4

## 15.0.15

### Patch Changes

- Updated dependencies [dbac0ca01]
  - @pnpm/local-resolver@8.0.11
  - @pnpm/npm-resolver@13.1.3

## 15.0.14

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/resolver-base@9.1.0
  - @pnpm/git-resolver@6.1.4
  - @pnpm/local-resolver@8.0.10
  - @pnpm/npm-resolver@13.1.2
  - @pnpm/tarball-resolver@6.0.7

## 15.0.13

### Patch Changes

- Updated dependencies [238a165a5]
  - @pnpm/npm-resolver@13.1.1

## 15.0.12

### Patch Changes

- Updated dependencies [39c040127]
  - @pnpm/git-resolver@6.1.3
  - @pnpm/local-resolver@8.0.9
  - @pnpm/npm-resolver@13.1.0

## 15.0.11

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/npm-resolver@13.1.0
  - @pnpm/local-resolver@8.0.8
  - @pnpm/resolver-base@9.0.6
  - @pnpm/git-resolver@6.1.2
  - @pnpm/tarball-resolver@6.0.6

## 15.0.10

### Patch Changes

- Updated dependencies [eb2426cf8]
  - @pnpm/npm-resolver@13.0.7
  - @pnpm/local-resolver@8.0.7

## 15.0.9

### Patch Changes

- @pnpm/git-resolver@6.1.1
- @pnpm/npm-resolver@13.0.6

## 15.0.8

### Patch Changes

- Updated dependencies [449ccef09]
  - @pnpm/git-resolver@6.1.0

## 15.0.7

### Patch Changes

- @pnpm/local-resolver@8.0.6
- @pnpm/npm-resolver@13.0.6
- @pnpm/resolver-base@9.0.5
- @pnpm/git-resolver@6.0.6
- @pnpm/tarball-resolver@6.0.5

## 15.0.6

### Patch Changes

- @pnpm/local-resolver@8.0.5
- @pnpm/npm-resolver@13.0.5
- @pnpm/resolver-base@9.0.4
- @pnpm/git-resolver@6.0.5
- @pnpm/tarball-resolver@6.0.4

## 15.0.5

### Patch Changes

- @pnpm/local-resolver@8.0.4
- @pnpm/npm-resolver@13.0.4
- @pnpm/resolver-base@9.0.3
- @pnpm/git-resolver@6.0.4
- @pnpm/tarball-resolver@6.0.3

## 15.0.4

### Patch Changes

- @pnpm/local-resolver@8.0.3
- @pnpm/npm-resolver@13.0.3
- @pnpm/resolver-base@9.0.2
- @pnpm/git-resolver@6.0.3
- @pnpm/tarball-resolver@6.0.2

## 15.0.3

### Patch Changes

- Updated dependencies [0fa446d10]
  - @pnpm/git-resolver@6.0.2

## 15.0.2

### Patch Changes

- @pnpm/local-resolver@8.0.2
- @pnpm/npm-resolver@13.0.2
- @pnpm/resolver-base@9.0.1
- @pnpm/git-resolver@6.0.1
- @pnpm/tarball-resolver@6.0.1

## 15.0.1

### Patch Changes

- @pnpm/error@3.0.1
- @pnpm/local-resolver@8.0.1
- @pnpm/npm-resolver@13.0.1

## 15.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [9c22c063e]
- Updated dependencies [542014839]
  - @pnpm/local-resolver@8.0.0
  - @pnpm/error@3.0.0
  - @pnpm/fetching-types@3.0.0
  - @pnpm/git-resolver@6.0.0
  - @pnpm/npm-resolver@13.0.0
  - @pnpm/resolver-base@9.0.0
  - @pnpm/tarball-resolver@6.0.0

## 14.0.12

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/local-resolver@7.0.8
  - @pnpm/npm-resolver@12.1.8

## 14.0.11

### Patch Changes

- @pnpm/local-resolver@7.0.7
- @pnpm/npm-resolver@12.1.7
- @pnpm/resolver-base@8.1.6
- @pnpm/git-resolver@5.1.17
- @pnpm/tarball-resolver@5.0.11

## 14.0.10

### Patch Changes

- @pnpm/local-resolver@7.0.6
- @pnpm/npm-resolver@12.1.6
- @pnpm/resolver-base@8.1.5
- @pnpm/git-resolver@5.1.16
- @pnpm/tarball-resolver@5.0.10

## 14.0.9

### Patch Changes

- @pnpm/local-resolver@7.0.5
- @pnpm/npm-resolver@12.1.5
- @pnpm/resolver-base@8.1.4
- @pnpm/git-resolver@5.1.15
- @pnpm/tarball-resolver@5.0.9

## 14.0.8

### Patch Changes

- @pnpm/local-resolver@7.0.4
- @pnpm/npm-resolver@12.1.4
- @pnpm/resolver-base@8.1.3
- @pnpm/git-resolver@5.1.14
- @pnpm/tarball-resolver@5.0.8

## 14.0.7

### Patch Changes

- Updated dependencies [c94104472]
- Updated dependencies [81ed15666]
  - @pnpm/git-resolver@5.1.13
  - @pnpm/npm-resolver@12.1.3
  - @pnpm/local-resolver@7.0.3
  - @pnpm/resolver-base@8.1.2
  - @pnpm/tarball-resolver@5.0.7

## 14.0.6

### Patch Changes

- @pnpm/git-resolver@5.1.12
- @pnpm/npm-resolver@12.1.2

## 14.0.5

### Patch Changes

- @pnpm/git-resolver@5.1.11
- @pnpm/npm-resolver@12.1.2

## 14.0.4

### Patch Changes

- Updated dependencies [9f61bd81b]
  - @pnpm/npm-resolver@12.1.2

## 14.0.3

### Patch Changes

- Updated dependencies [631877ebf]
  - @pnpm/local-resolver@7.0.2

## 14.0.2

### Patch Changes

- Updated dependencies [108bd4a39]
  - @pnpm/local-resolver@7.0.1
  - @pnpm/npm-resolver@12.1.1
  - @pnpm/resolver-base@8.1.1
  - @pnpm/git-resolver@5.1.10
  - @pnpm/tarball-resolver@5.0.6

## 14.0.1

### Patch Changes

- Updated dependencies [7da65bd7a]
  - @pnpm/git-resolver@5.1.9

## 14.0.0

### Major Changes

- 4ab87844a: Local directory dependencies are resolved to absolute path.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/local-resolver@7.0.0
  - @pnpm/npm-resolver@12.1.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/git-resolver@5.1.8
  - @pnpm/tarball-resolver@5.0.5

## 13.0.9

### Patch Changes

- @pnpm/git-resolver@5.1.7
- @pnpm/npm-resolver@12.0.5

## 13.0.8

### Patch Changes

- Updated dependencies [82caa0b56]
  - @pnpm/npm-resolver@12.0.5

## 13.0.7

### Patch Changes

- Updated dependencies [930e104da]
  - @pnpm/git-resolver@5.1.6
  - @pnpm/npm-resolver@12.0.4

## 13.0.6

### Patch Changes

- Updated dependencies [04b7f6086]
  - @pnpm/git-resolver@5.1.5

## 13.0.5

### Patch Changes

- Updated dependencies [bab172385]
  - @pnpm/fetching-types@2.2.1
  - @pnpm/git-resolver@5.1.4
  - @pnpm/npm-resolver@12.0.4

## 13.0.4

### Patch Changes

- Updated dependencies [eadf0e505]
- Updated dependencies [eadf0e505]
  - @pnpm/fetching-types@2.2.0
  - @pnpm/npm-resolver@12.0.3
  - @pnpm/git-resolver@5.1.3

## 13.0.3

### Patch Changes

- Updated dependencies [3f0178b4c]
  - @pnpm/local-resolver@6.1.0

## 13.0.2

### Patch Changes

- Updated dependencies [a4fed2798]
  - @pnpm/npm-resolver@12.0.2

## 13.0.1

### Patch Changes

- @pnpm/local-resolver@6.0.5
- @pnpm/npm-resolver@12.0.1
- @pnpm/resolver-base@8.0.4
- @pnpm/git-resolver@5.1.2
- @pnpm/tarball-resolver@5.0.4

## 13.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [691f64713]
  - @pnpm/npm-resolver@12.0.0

## 12.0.7

### Patch Changes

- @pnpm/local-resolver@6.0.4
- @pnpm/npm-resolver@11.1.4
- @pnpm/resolver-base@8.0.3
- @pnpm/git-resolver@5.1.1
- @pnpm/tarball-resolver@5.0.3

## 12.0.6

### Patch Changes

- Updated dependencies [69ffc4099]
  - @pnpm/git-resolver@5.1.0

## 12.0.5

### Patch Changes

- @pnpm/git-resolver@5.0.2
- @pnpm/npm-resolver@11.1.3
- @pnpm/local-resolver@6.0.3
- @pnpm/resolver-base@8.0.2
- @pnpm/tarball-resolver@5.0.2

## 12.0.4

### Patch Changes

- Updated dependencies [ae36ac7d3]
- Updated dependencies [bf322c702]
  - @pnpm/npm-resolver@11.1.2

## 12.0.3

### Patch Changes

- @pnpm/local-resolver@6.0.2
- @pnpm/npm-resolver@11.1.1

## 12.0.2

### Patch Changes

- Updated dependencies [85fb21a83]
- Updated dependencies [05baaa6e7]
  - @pnpm/npm-resolver@11.1.0
  - @pnpm/fetching-types@2.1.0
  - @pnpm/local-resolver@6.0.1
  - @pnpm/git-resolver@5.0.1
  - @pnpm/resolver-base@8.0.1
  - @pnpm/tarball-resolver@5.0.1

## 12.0.1

### Patch Changes

- Updated dependencies [6f198457d]
  - @pnpm/npm-resolver@11.0.1

## 12.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [83645c8ed]
- Updated dependencies [992820161]
  - @pnpm/error@2.0.0
  - @pnpm/fetching-types@2.0.0
  - @pnpm/git-resolver@5.0.0
  - @pnpm/local-resolver@6.0.0
  - @pnpm/npm-resolver@11.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/tarball-resolver@5.0.0

## 11.0.20

### Patch Changes

- @pnpm/git-resolver@4.1.12
- @pnpm/npm-resolver@10.2.2

## 11.0.19

### Patch Changes

- Updated dependencies [a00ee0035]
  - @pnpm/tarball-resolver@4.0.8

## 11.0.18

### Patch Changes

- Updated dependencies [ad113645b]
  - @pnpm/local-resolver@5.1.3

## 11.0.17

### Patch Changes

- @pnpm/local-resolver@5.1.2
- @pnpm/npm-resolver@10.2.2
- @pnpm/resolver-base@7.1.1
- @pnpm/git-resolver@4.1.11
- @pnpm/tarball-resolver@4.0.7

## 11.0.16

### Patch Changes

- Updated dependencies [32c9ef4be]
  - @pnpm/git-resolver@4.1.10

## 11.0.15

### Patch Changes

- Updated dependencies [f47551a3c]
  - @pnpm/npm-resolver@10.2.1

## 11.0.14

### Patch Changes

- @pnpm/git-resolver@4.1.9
- @pnpm/npm-resolver@10.2.0

## 11.0.13

### Patch Changes

- @pnpm/git-resolver@4.1.8
- @pnpm/npm-resolver@10.2.0

## 11.0.12

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/npm-resolver@10.2.0
  - @pnpm/resolver-base@7.1.0
  - @pnpm/git-resolver@4.1.7
  - @pnpm/local-resolver@5.1.1
  - @pnpm/tarball-resolver@4.0.6

## 11.0.11

### Patch Changes

- Updated dependencies [284e95c5e]
- Updated dependencies [284e95c5e]
- Updated dependencies [084614f55]
  - @pnpm/local-resolver@5.1.0
  - @pnpm/npm-resolver@10.1.0

## 11.0.10

### Patch Changes

- Updated dependencies [5ff6c28fa]
- Updated dependencies [0c5f1bcc9]
  - @pnpm/npm-resolver@10.0.7
  - @pnpm/error@1.4.0
  - @pnpm/local-resolver@5.0.20

## 11.0.9

### Patch Changes

- @pnpm/local-resolver@5.0.19

## 11.0.8

### Patch Changes

- Updated dependencies [39142e2ad]
  - @pnpm/npm-resolver@10.0.6
  - @pnpm/local-resolver@5.0.18

## 11.0.7

### Patch Changes

- @pnpm/local-resolver@5.0.17
- @pnpm/npm-resolver@10.0.5
- @pnpm/resolver-base@7.0.5
- @pnpm/git-resolver@4.1.6
- @pnpm/tarball-resolver@4.0.5

## 11.0.6

### Patch Changes

- @pnpm/local-resolver@5.0.16
- @pnpm/npm-resolver@10.0.4
- @pnpm/resolver-base@7.0.4
- @pnpm/git-resolver@4.1.5
- @pnpm/tarball-resolver@4.0.4

## 11.0.5

### Patch Changes

- @pnpm/local-resolver@5.0.15

## 11.0.4

### Patch Changes

- Updated dependencies [d7b727795]
  - @pnpm/npm-resolver@10.0.3

## 11.0.3

### Patch Changes

- Updated dependencies [3633f5e46]
  - @pnpm/npm-resolver@10.0.2
  - @pnpm/git-resolver@4.1.4

## 11.0.2

### Patch Changes

- @pnpm/git-resolver@4.1.3
- @pnpm/npm-resolver@10.0.1

## 11.0.1

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/npm-resolver@10.0.1
  - @pnpm/local-resolver@5.0.14

## 11.0.0

### Major Changes

- a1cdae3dc: Does not accept a `metaCache` option anymore. Caching happens internally, using `lru-cache`.

### Patch Changes

- Updated dependencies [a1cdae3dc]
  - @pnpm/npm-resolver@10.0.0

## 10.0.7

### Patch Changes

- Updated dependencies [6d480dd7a]
- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/npm-resolver@9.1.0
  - @pnpm/local-resolver@5.0.13
  - @pnpm/git-resolver@4.1.2

## 10.0.6

### Patch Changes

- @pnpm/local-resolver@5.0.12

## 10.0.5

### Patch Changes

- @pnpm/local-resolver@5.0.11

## 10.0.4

### Patch Changes

- Updated dependencies [622c0b6f9]
- Updated dependencies [a2ef8084f]
  - @pnpm/npm-resolver@9.0.2

## 10.0.3

### Patch Changes

- @pnpm/git-resolver@4.1.1
- @pnpm/npm-resolver@9.0.1

## 10.0.2

### Patch Changes

- Updated dependencies [379cdcaf8]
- Updated dependencies [7b98d16c8]
- Updated dependencies [2ebcfc38a]
  - @pnpm/npm-resolver@9.0.1
  - @pnpm/git-resolver@4.1.0

## 10.0.1

### Patch Changes

- Updated dependencies [83b146d63]
  - @pnpm/tarball-resolver@4.0.3

## 10.0.0

### Major Changes

- 71aeb9a38: Breaking changes to the API. fetchFromRegistry and getCredentials are passed in through arguments.

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [71aeb9a38]
  - @pnpm/fetching-types@1.0.0
  - @pnpm/npm-resolver@9.0.0
  - @pnpm/git-resolver@4.0.16

## 9.0.3

### Patch Changes

- @pnpm/local-resolver@5.0.10
- @pnpm/npm-resolver@8.1.2
- @pnpm/resolver-base@7.0.3
- @pnpm/git-resolver@4.0.15
- @pnpm/tarball-resolver@4.0.2

## 9.0.2

### Patch Changes

- Updated dependencies [1520e3d6f]
  - @pnpm/local-resolver@5.0.9

## 9.0.1

### Patch Changes

- @pnpm/local-resolver@5.0.8
- @pnpm/npm-resolver@8.1.1
- @pnpm/resolver-base@7.0.2
- @pnpm/git-resolver@4.0.14
- @pnpm/tarball-resolver@4.0.1

## 9.0.0

### Major Changes

- 41d92948b: The direct tarball dependency ID starts with a @ and the tarball extension is not removed.

### Patch Changes

- Updated dependencies [41d92948b]
  - @pnpm/tarball-resolver@4.0.0
  - @pnpm/local-resolver@5.0.7

## 8.0.2

### Patch Changes

- Updated dependencies [4cf7ef367]
- Updated dependencies [d3ddd023c]
  - @pnpm/npm-resolver@8.1.0
  - @pnpm/git-resolver@4.0.13

## 8.0.1

### Patch Changes

- @pnpm/npm-resolver@8.0.1

## 8.0.0

### Patch Changes

- Updated dependencies [5bc033c43]
- Updated dependencies [f453a5f46]
  - @pnpm/npm-resolver@8.0.0
  - @pnpm/error@1.2.1
  - @pnpm/git-resolver@4.0.12
  - @pnpm/local-resolver@5.0.6
  - @pnpm/resolver-base@7.0.1
  - @pnpm/tarball-resolver@3.0.5

## 7.4.10-alpha.2

### Patch Changes

- @pnpm/local-resolver@5.0.6-alpha.0
- @pnpm/npm-resolver@7.3.12-alpha.2
- @pnpm/resolver-base@7.0.1-alpha.0
- @pnpm/git-resolver@4.0.12-alpha.0
- @pnpm/tarball-resolver@3.0.5-alpha.0

## 7.4.10-alpha.1

### Patch Changes

- Updated dependencies [5bc033c43]
  - @pnpm/npm-resolver@8.0.0-alpha.1

## 7.4.10-alpha.0

### Patch Changes

- Updated dependencies [f453a5f46]
  - @pnpm/npm-resolver@7.3.12-alpha.0

## 7.4.9

### Patch Changes

- @pnpm/local-resolver@5.0.5
