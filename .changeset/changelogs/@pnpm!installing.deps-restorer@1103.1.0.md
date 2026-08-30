## 1103.1.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

### Patch Changes

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.29
  - @pnpm/building.during-install@1102.1.0
  - @pnpm/building.policy@1100.0.21
  - @pnpm/config.normalize-registries@1101.0.1
  - @pnpm/config.package-is-installable@1100.1.5
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.graph-builder@1101.0.2
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/deps.path@1101.0.1
  - @pnpm/exec.lifecycle@1100.1.15
  - @pnpm/fs.symlink-dependency@1100.0.19
  - @pnpm/installing.linking.direct-dep-linker@1100.0.19
  - @pnpm/installing.linking.hoist@1100.0.29
  - @pnpm/installing.linking.modules-cleaner@1100.1.21
  - @pnpm/installing.linking.real-hoist@1100.1.16
  - @pnpm/installing.modules-yaml@1101.0.1
  - @pnpm/installing.package-requester@1102.1.12
  - @pnpm/lockfile.filtering@1100.2.5
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.to-pnp@1101.0.2
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/patching.config@1100.1.3
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/pnpr.client@3.0.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.project-manifest-reader@1100.0.26
