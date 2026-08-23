## 1100.2.0

### Minor Changes

- An install that had to re-hash store files to verify them now reports it. If that cost more than a second, it says how long — `The integrity of N files was checked in 2.5s.` — and if it was quick but covered more than a thousand files, it names the cause instead: their timestamps changed since the store recorded them, which a backup tool, an antivirus scan or a copied store can do.

### Patch Changes

- Updated dependencies:
  - @pnpm/error@1100.1.3
  - @pnpm/fetching.fetcher-base@1100.2.8
  - @pnpm/store.controller-types@1101.1.2
  - @pnpm/types@1102.0.0
