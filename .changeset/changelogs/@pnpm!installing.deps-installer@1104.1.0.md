## 1104.1.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Verified remote build artifacts are persisted in the shared store with their signed origin metadata. Later installs reverify the artifact against current trust, policy, platform, and source before reuse, while invalid remote variants are quarantined per channel ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).

- Allowed `pnpm update --patches` to refresh registry revisions through a configured pnpr server while retaining locked package versions.

- Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.

### Patch Changes

- `pnpm update` no longer moves the range a project declares for a dependency that `overrides` also lists, even when the override repeats that range verbatim. Previously the updated `package.json` disagreed with the lockfile, so the next `pnpm install --frozen-lockfile` failed with a specifier mismatch [#14224](https://github.com/pnpm/pnpm/issues/14224).

- Make `pnpm add --lockfile-only` skip dependency linking [pnpm/pnpm#14286](https://github.com/pnpm/pnpm/issues/14286).

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- Forward `patchedDependencies` hashes and `packageExtensions` to pnpr so server-side resolution preserves patches and package extensions in the lockfile and installed packages.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.29
  - @pnpm/bins.remover@1100.0.22
  - @pnpm/building.after-install@1103.0.2
  - @pnpm/building.during-install@1102.1.0
  - @pnpm/building.policy@1100.0.21
  - @pnpm/config.normalize-registries@1101.0.1
  - @pnpm/config.version-policy@1100.2.2
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/crypto.hash@1100.0.3
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/deps.graph-sequencer@1101.0.0
  - @pnpm/deps.path@1101.0.1
  - @pnpm/exec.lifecycle@1100.1.15
  - @pnpm/fs.symlink-dependency@1100.0.19
  - @pnpm/hooks.read-package-hook@1100.3.0
  - @pnpm/hooks.types@1101.0.1
  - @pnpm/installing.context@1101.0.2
  - @pnpm/installing.deps-resolver@1102.1.0
  - @pnpm/installing.deps-restorer@1103.1.0
  - @pnpm/installing.linking.direct-dep-linker@1100.0.19
  - @pnpm/installing.linking.hoist@1100.0.29
  - @pnpm/installing.linking.modules-cleaner@1100.1.21
  - @pnpm/installing.modules-yaml@1101.0.1
  - @pnpm/installing.package-requester@1102.1.12
  - @pnpm/lockfile.filtering@1100.2.5
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.preferred-versions@1100.0.31
  - @pnpm/lockfile.pruner@1100.0.21
  - @pnpm/lockfile.settings-checker@1100.2.4
  - @pnpm/lockfile.to-pnp@1101.0.2
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/lockfile.verification@1100.1.3
  - @pnpm/lockfile.walker@1100.0.21
  - @pnpm/network.auth-header@1101.1.12
  - @pnpm/patching.config@1100.1.3
  - @pnpm/pkg-manifest.utils@1100.4.2
  - @pnpm/pnpr.client@3.0.0
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/store.index@1100.3.0
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.project-manifest-reader@1100.0.26
