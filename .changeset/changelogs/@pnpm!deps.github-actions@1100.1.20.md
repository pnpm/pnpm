## 1100.1.20

### Patch Changes

- Fixed GitHub Actions support in `pnpm update` and `pnpm outdated`. Updating a workflow step with a major or minor version comment such as `# v4` or a prose comment now formats the updated version comment cleanly. Negated action selectors such as `!actions/cache/save` now exclude only the targeted subpath actions. Passing only negated action selectors still updates packages. Previously, negated action selectors skipped package updates. `pnpm outdated` now reports every distinct outdated version when several workflows pin the same action at different versions.

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.11
  - @pnpm/resolving.git-resolver@1100.1.31
  - @pnpm/types@1102.1.4
