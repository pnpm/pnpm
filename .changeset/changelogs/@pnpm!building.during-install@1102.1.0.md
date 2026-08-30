## 1102.1.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

### Patch Changes

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.29
  - @pnpm/config.reader@1102.1.0
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/deps.path@1101.0.1
  - @pnpm/exec.lifecycle@1100.1.15
  - @pnpm/fs.hard-link-dir@1100.0.4
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/pnpr.client@3.0.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/types@1102.1.0
