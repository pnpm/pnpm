# @pnpm/deps.security.signatures

## 1101.2.3

### Patch Changes

- Updated dependencies [05b95ab]
- Updated dependencies [852d537]
  - @pnpm/network.fetch@1100.1.4
  - @pnpm/error@1100.0.1

## 1101.2.2

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
  - @pnpm/fetching.types@1100.0.2
  - @pnpm/network.fetch@1100.1.3

## 1101.2.1

### Patch Changes

- @pnpm/network.fetch@1100.1.2

## 1101.2.0

### Minor Changes

- 5f2bb9f: Security: pnpm now verifies the npm registry signature of a package-manager binary before spawning it, so a cloned repository cannot make pnpm download and execute an arbitrary native binary.

  This covers two paths that select an executable from repository-controlled input:

  - **pacquet install engine** — declaring `pacquet` (or `@pnpm/pacquet`) in `configDependencies` opts in to pnpm's Rust install engine. pnpm now verifies that the installed `pacquet` shim and the host's `@pacquet/<platform>-<arch>` binary carry a valid npm registry signature for their exact `name@version`, and refuses to run pacquet (failing the command) if the signature does not verify or cannot be checked. The only graceful fallback to pnpm's own engine is when pacquet has no binary for the current platform.
  - **automatic version switch / `self-update`** — the `packageManager` / `devEngines.packageManager` field makes pnpm download and run a specific pnpm version. pnpm now verifies the registry signature of `pnpm`, `@pnpm/exe`, and the host platform binary before installing/spawning them, and refuses to run an engine whose signature does not match a published, signed release. The check runs only on an actual download (store cache miss), so it does not add a network round trip to every command.

  In both cases the signature is verified over the _installed_ integrity, against npm's public signing keys that ship embedded in the pnpm CLI (like corepack), so bytes substituted via a tampered lockfile or a repository-controlled registry fail verification — and a registry the user did not vouch for cannot supply its own signing keys. The signed packument is fetched from the configured registry, so an npm mirror works transparently. Verification fails closed: if it cannot be completed (for example, the registry is unreachable), the command fails rather than running an unverified binary. The embedded keys are kept current by a release-time check against npm's signing-keys endpoint.

### Patch Changes

- @pnpm/network.fetch@1100.1.1

## 1101.1.6

### Patch Changes

- Updated dependencies [60a1eec]
  - @pnpm/network.fetch@1100.1.0

## 1101.1.5

### Patch Changes

- Updated dependencies [b1fa2d5]
  - @pnpm/network.fetch@1100.0.8

## 1101.1.4

### Patch Changes

- @pnpm/network.fetch@1100.0.7

## 1101.1.3

### Patch Changes

- @pnpm/network.fetch@1100.0.6

## 1101.1.2

### Patch Changes

- @pnpm/network.fetch@1100.0.5

## 1101.1.1

### Patch Changes

- Updated dependencies [18a464f]
  - @pnpm/network.fetch@1100.0.4

## 1101.1.0

### Minor Changes

- 6ac06cb: Added `pnpm audit signatures` to verify ECDSA registry signatures for installed packages against keys from `/-/npm/v1/keys` [#7909](https://github.com/pnpm/pnpm/issues/7909). Scoped registries are respected, and registries without signing keys are skipped.

### Patch Changes

- Updated dependencies [20e7aff]
  - @pnpm/network.fetch@1100.0.3
