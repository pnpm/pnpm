## 1100.3.0

### Minor Changes

- An install that had to re-hash store files to verify them now reports it. If that cost more than a second, it says how long — `The integrity of N files was checked in 2.5s.` — and if it was quick but covered more than a thousand files, it names the cause instead: their timestamps changed since the store recorded them, which a backup tool, an antivirus scan or a copied store can do.

### Patch Changes

- Fixed `pnpm install` sometimes not exiting after printing `Done in Xs` [#12297](https://github.com/pnpm/pnpm/issues/12297).

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.15
  - @pnpm/crypto.integrity@1100.0.5
  - @pnpm/error@1100.1.3
  - @pnpm/fs.symlink-dependency@1100.0.18
  - @pnpm/store.cafs@1100.2.0
  - @pnpm/store.create-cafs-store@1100.0.26
  - @pnpm/store.index@1100.2.5
