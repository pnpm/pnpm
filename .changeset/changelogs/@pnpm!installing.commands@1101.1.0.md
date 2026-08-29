## 1101.1.0

### Minor Changes

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.

- `sideEffectsCache` now declares the whole of how a package's build output is reused — whether one is restored, whether one is saved, and the remote tier that shares it between machines:

  ```yaml
  sideEffectsCache:
    read: true
    write: true
    remote:
      org: acme
      packages: ['native-addon']
  ```

  `sideEffectsCache: true`, `sideEffectsCacheReadonly`, `remoteSideEffectsCache`, and its `organization` field all keep working. Where a field is set under both spellings the one above wins; where it is set under only one, it is kept.

  Two behaviors change, both bringing this CLI in line with what the Rust one already did: `sideEffectsCacheReadonly: true` now blocks writing to the cache, and setting it alongside `sideEffectsCache: false` gives a read-only view rather than switching the cache off entirely. A cache can also be declared write-only now, to populate one the run does not read.

### Patch Changes

- The options type of the `fetch` command now declares `allowBuilds`, a setting its handler already forwarded to the installer. Type-level only — what `pnpm fetch` does is unchanged.

- `pnpm update -g` no longer downgrades a global package. `--latest` resolves the `latest` dist-tag, which can point at an older release than the one installed — after `pnpm add -g <pkg>@next`, for instance [#14270](https://github.com/pnpm/pnpm/issues/14270).

  `pnpm update -g` also no longer changes the pnpm version. pnpm's own global install belongs to `pnpm self-update` [#14270](https://github.com/pnpm/pnpm/issues/14270).

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- `pnpm stage approve` now approves several staged packages at once. Run it without a stage id to pick from the staged versions interactively, or pass a list of stage ids. The whole batch is approved with a single one-time password, and pnpm asks for a new one only once the registry stops accepting it. Inside a workspace, the selected packages are approved in dependency order, and a package whose workspace dependency could not be approved is skipped instead of being published against a dependency that never reached the registry.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.2
  - @pnpm/building.policy@1100.0.21
  - @pnpm/cli.utils@1101.0.25
  - @pnpm/config.pick-registry-for-package@1101.0.1
  - @pnpm/config.reader@1102.1.0
  - @pnpm/config.version-policy@1100.2.2
  - @pnpm/config.writer@1100.0.24
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.github-actions@1100.1.7
  - @pnpm/deps.inspection.outdated@1100.1.28
  - @pnpm/deps.path@1101.0.1
  - @pnpm/deps.security.signatures@1102.0.1
  - @pnpm/deps.status@1100.1.20
  - @pnpm/fs.graceful-fs@1100.2.0
  - @pnpm/global.commands@1101.1.0
  - @pnpm/global.packages@1101.1.0
  - @pnpm/hooks.pnpmfile@1100.0.29
  - @pnpm/installing.context@1101.0.2
  - @pnpm/installing.dedupe.check@1100.1.11
  - @pnpm/installing.deps-installer@1104.1.0
  - @pnpm/installing.env-installer@1103.0.2
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/network.auth-header@1101.1.12
  - @pnpm/network.fetch@1100.1.14
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/pkg-manifest.utils@1100.4.2
  - @pnpm/resolving.npm-resolver@1104.1.0
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/store.connection-manager@1101.1.0
  - @pnpm/store.controller@1102.1.0
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.project-manifest-reader@1100.0.26
  - @pnpm/workspace.project-manifest-writer@1100.0.16
  - @pnpm/workspace.projects-filter@1100.0.40
  - @pnpm/workspace.projects-graph@1100.0.35
  - @pnpm/workspace.projects-reader@1101.0.25
  - @pnpm/workspace.projects-sorter@1101.0.0
  - @pnpm/workspace.state@1100.0.41
  - @pnpm/workspace.workspace-manifest-reader@1100.1.8
  - @pnpm/workspace.workspace-manifest-writer@1100.1.2
