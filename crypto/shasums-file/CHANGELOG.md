# @pnpm/crypto.shasums-file

## 1001.0.6

### Patch Changes

- c452019: pnpm now verifies the detached OpenPGP signature of a Node.js release's `SHASUMS256.txt` against the Node.js release team's public keys (embedded in the pnpm CLI) before trusting its hashes. The Node.js download mirror is repository-configurable (`node-mirror:<channel>` in `.npmrc`), and the integrity check previously trusted a `SHASUMS256.txt` fetched from that same mirror — a circular check that a malicious mirror could satisfy with a tampered binary and matching hashes. A mirror that proxies the real signed SHASUMS keeps working unchanged. Only the `release` channel publishes signed SHASUMS files, so pre-release channels (rc, nightly, …) remain unverified.
  - @pnpm/crypto.hash@1000.2.2

## 1001.0.5

### Patch Changes

- Updated dependencies [523f816]
  - @pnpm/error@1000.1.0
  - @pnpm/crypto.hash@1000.2.2

## 1001.0.4

### Patch Changes

- @pnpm/crypto.hash@1000.2.2

## 1001.0.3

### Patch Changes

- Updated dependencies [a484cea]
  - @pnpm/fetching-types@1000.2.1
  - @pnpm/crypto.hash@1000.2.1

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
