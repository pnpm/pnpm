## 1101.0.19

### Patch Changes

- `pnpm patch-remove` now removes the corresponding edit directory and state entry from `node_modules/.pnpm_patches`.

- `pnpm patch-commit` now keeps a file without a final newline unterminated when that file comes last in the patch. Previously the patch dropped the final `\ No newline at end of file` marker, so the patched file gained a trailing newline.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.reader@1102.4.0
  - @pnpm/config.writer@1100.0.39
  - @pnpm/crypto.hash@1100.0.10
  - @pnpm/error@1100.2.3
  - @pnpm/fs.packlist@1100.0.15
  - @pnpm/installing.commands@1101.4.13
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.pruner@1100.0.32
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/patching.apply-patch@1100.0.13
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/store.connection-manager@1101.3.11
  - @pnpm/store.path@1100.1.4
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
  - @pnpm/workspace.workspace-manifest-reader@1100.3.4
