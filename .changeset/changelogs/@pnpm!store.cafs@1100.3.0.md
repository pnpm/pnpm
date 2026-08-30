## 1100.3.0

### Minor Changes

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

- Restoring a dependency's build from the remote side-effects cache no longer downloads files the store already holds.

### Patch Changes

- Enforce `allowBuilds` when a prepared git dependency is reused from the shared store, and use the lockfile's canonical git resolution ID in approval suggestions.

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.9
  - @pnpm/fs.graceful-fs@1100.2.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/types@1102.1.0
