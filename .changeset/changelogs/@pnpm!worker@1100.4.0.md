## 1100.4.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

### Patch Changes

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

- Enforce `allowBuilds` when a prepared git dependency is reused from the shared store, and use the lockfile's canonical git resolution ID in approval suggestions.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.16
  - @pnpm/fs.graceful-fs@1100.2.0
  - @pnpm/fs.hard-link-dir@1100.0.4
  - @pnpm/fs.symlink-dependency@1100.0.19
  - @pnpm/store.cafs@1100.3.0
  - @pnpm/store.cafs-types@1100.1.0
  - @pnpm/store.create-cafs-store@1100.0.27
  - @pnpm/store.index@1100.3.0
