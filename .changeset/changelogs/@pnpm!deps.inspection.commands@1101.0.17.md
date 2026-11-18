## 1101.0.17

### Patch Changes

- Fixed GitHub Actions support in `pnpm update` and `pnpm outdated`. Updating a workflow step with a major or minor version comment such as `# v4` or a prose comment now formats the updated version comment cleanly. Negated action selectors such as `!actions/cache/save` now exclude only the targeted subpath actions. Passing only negated action selectors still updates packages. Previously, negated action selectors skipped package updates. `pnpm outdated` now reports every distinct outdated version when several workflows pin the same action at different versions.

- `pnpm peers` in workspaces with `sharedWorkspaceLockfile: false` now checks each selected project's `pnpm-lock.yaml`.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.pick-registry-for-package@1101.0.6
  - @pnpm/config.reader@1102.3.10
  - @pnpm/deps.github-actions@1100.1.20
  - @pnpm/deps.inspection.list@1101.1.5
  - @pnpm/deps.inspection.outdated@1100.1.43
  - @pnpm/deps.inspection.peers-checker@1100.0.45
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.18
  - @pnpm/global.commands@1102.0.14
  - @pnpm/global.packages@1101.1.12
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/network.auth-header@1101.1.19
  - @pnpm/network.fetch@1100.1.26
  - @pnpm/resolving.default-resolver@1101.0.17
  - @pnpm/resolving.npm-resolver@1104.2.11
  - @pnpm/resolving.registry.types@1100.2.6
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
