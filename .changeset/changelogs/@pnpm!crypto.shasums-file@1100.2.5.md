## 1100.2.5

### Patch Changes

- Resolving a Node.js runtime now fails when unofficial-builds.nodejs.org cannot be reached. pnpm used to ignore that failure and leave the musl builds out of `pnpm-lock.yaml`. `pnpm update` then wrote a different lockfile on a machine whose network blocks the mirror [pnpm/pnpm#14813](https://github.com/pnpm/pnpm/issues/14813).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.5
