## 1100.3.0

### Minor Changes

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).
