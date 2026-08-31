## 1102.1.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Restoring a dependency's build from the remote side-effects cache no longer downloads files the store already holds.

### Patch Changes

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.3
  - @pnpm/fetching.fetcher-base@1100.2.9
  - @pnpm/hooks.types@1101.0.1
  - @pnpm/installing.package-requester@1102.1.12
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/store.cafs@1100.3.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/store.create-cafs-store@1100.0.27
  - @pnpm/store.index@1100.3.0
  - @pnpm/types@1102.1.0
