## 1100.1.21

### Patch Changes

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` corrupting workflow YAML or skipping actions referenced through YAML anchors, aliases, or merge keys.

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` matching every package dependency when combining a GitHub Action selector with a negated package selector such as `!lodash`.

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.12
  - @pnpm/error@1100.2.3
  - @pnpm/resolving.git-resolver@1100.1.32
  - @pnpm/types@1102.1.5
