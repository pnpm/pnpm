# @pnpm/crypto.shasums-file

## 1100.1.2

### Patch Changes

- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1
  - @pnpm/crypto.hash@1100.0.1

## 1100.1.1

### Patch Changes

- Updated dependencies [681b593]
  - @pnpm/fetching.types@1100.0.2
  - @pnpm/crypto.hash@1100.0.1

## 1100.1.0

### Minor Changes

- 3d50680: Security: pnpm now verifies the OpenPGP signature of a downloaded Node.js runtime's `SHASUMS256.txt` before trusting its integrity hashes.

  When a repository requests a Node.js runtime (e.g. via `devEngines.runtime` / `useNodeVersion`), the download mirror is repository-configurable through `node-mirror:<channel>`. The integrity of the downloaded binary was only checked against `SHASUMS256.txt` fetched from that same mirror — a circular check that a malicious mirror could satisfy by serving a tampered binary together with a matching `SHASUMS256.txt`. pnpm then executes the binary (for example to run lifecycle scripts).

  pnpm now fetches `SHASUMS256.txt.sig` and verifies the detached OpenPGP signature against the Node.js release team's public keys, which ship embedded in the pnpm CLI. A mirror that serves a tampered binary cannot also produce a valid signature, so the download fails to verify. The embedded keys are kept current by a release-time check against the canonical `nodejs/release-keys` list.

  The musl variants from the hardcoded `unofficial-builds.nodejs.org` mirror are not repository-configurable and are signed by a different key, so they continue to be trusted over TLS.

### Patch Changes

- @pnpm/crypto.hash@1100.0.1

## 1100.0.1

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/fetching.types@1100.0.1
  - @pnpm/crypto.hash@1100.0.1

## 1002.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- Updated dependencies [491a84f]
- Updated dependencies [bb8baa7]
- Updated dependencies [7d2fd48]
- Updated dependencies [6c480a4]
- Updated dependencies [831f574]
  - @pnpm/fetching.types@1001.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/crypto.hash@1001.0.0

## 1001.0.2

### Patch Changes

- @pnpm/crypto.hash@1000.2.1

## 1001.0.1

### Patch Changes

- @pnpm/error@1000.0.5
- @pnpm/crypto.hash@1000.2.0

## 1001.0.0

### Major Changes

- 86b33e9: fetchShasumsFile returns an array of shasum file items.

### Patch Changes

- @pnpm/error@1000.0.4
- @pnpm/crypto.hash@1000.2.0

## 1000.0.0

### Major Changes

- 1a07b8f: Initial release.

### Patch Changes

- Updated dependencies [1ba2e15]
  - @pnpm/fetching-types@1000.2.0
  - @pnpm/error@1000.0.3
  - @pnpm/crypto.hash@1000.2.0
