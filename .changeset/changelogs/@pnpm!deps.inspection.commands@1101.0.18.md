## 1101.0.18

### Patch Changes

- `pnpm peers check` now resolves the `catalog:` peer ranges of injected workspace packages through the workspace catalogs. With `autoInstallPeers: false`, such a peer was reported as unmet even when the installed version matched the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).

- `pnpm list --lockfile-only --find-by` now reads manifests from the configured store when `node_modules` is absent.

- `pnpm list -g` with `--depth` greater than `0` now always reads the global install directory's `node_modules` and lockfile. Previously it could read the current project's lockfile or a custom `modulesDir`.

- Global commands (`--global`, `-g`) no longer load `.pnpmfile.cjs`, `pnpm-workspace.yaml`, or `.npmrc` from the current working directory. `pnpm why -g` now inspects globally installed packages.

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` matching every package dependency when combining a GitHub Action selector with a negated package selector such as `!lodash`.

- `pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. Previously the flag was accepted but ignored.

- Updated dependencies:
  - @pnpm/cli.common-cli-options-help@1100.0.4
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.pick-registry-for-package@1101.0.7
  - @pnpm/config.reader@1102.4.0
  - @pnpm/deps.github-actions@1100.1.21
  - @pnpm/deps.inspection.list@1101.1.6
  - @pnpm/deps.inspection.outdated@1100.1.44
  - @pnpm/deps.inspection.peers-checker@1100.0.46
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.19
  - @pnpm/error@1100.2.3
  - @pnpm/global.commands@1102.0.15
  - @pnpm/global.packages@1101.1.13
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/network.auth-header@1101.1.20
  - @pnpm/network.fetch@1100.1.27
  - @pnpm/resolving.default-resolver@1101.0.18
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/resolving.registry.types@1100.2.7
  - @pnpm/store.path@1100.1.4
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
