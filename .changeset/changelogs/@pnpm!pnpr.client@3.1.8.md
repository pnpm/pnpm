## 3.1.8

### Patch Changes

- pnpr now advertises capabilities through `pnpr.protocols` at the server root and npm registry bases. pnpm reads this capability map for remote resolution and shared build caches. The HTTP protocol path remains `/v0/`.

- Installs now fail when a pnpr server does not confirm the requested resolution and verification policies. Upgrade the pnpr server before upgrading pnpm.

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/store.cafs@1100.3.10
  - @pnpm/store.controller-types@1101.3.6
