## 1100.3.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

### Patch Changes

- Updated dependencies:
  - @pnpm/deps.path@1101.0.1
  - @pnpm/engine.runtime.system-version@1100.0.11
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/types@1102.1.0
