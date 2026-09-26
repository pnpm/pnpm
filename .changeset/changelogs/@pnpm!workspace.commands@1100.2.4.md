## 1100.2.4

### Patch Changes

- Fixed `pnpm version` failing on projects using a `package.yaml` manifest.

  Fixed `pnpm init` creating an extra `package.json` when `package.yaml` is already present.

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.1
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.reader@1102.3.0
  - @pnpm/engine.pm.commands@1102.1.4
  - @pnpm/error@1100.2.0
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-writer@1100.0.18
