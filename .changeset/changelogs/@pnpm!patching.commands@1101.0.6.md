## 1101.0.6

### Patch Changes

- `pnpm patch-commit` now falls back to copying package files when hard linking fails.

- `pnpm patch-commit` now resolves default patch directory locations when passed a package name or package specifier (such as `pnpm patch-commit <pkg>` or `pnpm patch-commit <pkg>@<version>`).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.reader@1102.3.0
  - @pnpm/config.writer@1100.0.28
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/error@1100.2.0
  - @pnpm/fs.packlist@1100.0.5
  - @pnpm/installing.commands@1101.4.0
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.pruner@1100.0.25
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/patching.apply-patch@1100.0.9
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/store.connection-manager@1101.2.0
  - @pnpm/store.path@1100.0.8
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
  - @pnpm/workspace.workspace-manifest-reader@1100.2.0
