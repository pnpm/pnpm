## 3.0.0

### Major Changes

- Generalized the experimental shared-artifact protocol so candidates and signed payloads identify a discriminated subject. Dependency side effects use package and source-integrity subjects, while workspace tasks use project and task subjects.

  This changes shared-artifact request bodies and signed payloads. A pnpr server and its clients have to be on matching versions.

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Added macOS and Windows x64 and arm64 support to remote shared build artifacts [pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771).

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

- Allowed `pnpm update --patches` to refresh registry revisions through a configured pnpr server while retaining locked package versions.

- Restoring a dependency's build from the remote side-effects cache no longer downloads files the store already holds.

### Patch Changes

- Forward `patchedDependencies` hashes and `packageExtensions` to pnpr so server-side resolution preserves patches and package extensions in the lockfile and installed packages.

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/network.auth-header@1101.1.12
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/types@1102.1.0
