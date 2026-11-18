## 1102.3.10

### Patch Changes

- `pnpm ci` now validates that `pnpm-lock.yaml` exists and matches `package.json` before removing `node_modules`. `pnpm clean` and `pnpm ci` now honor `--filter`, `--filter-prod`, and `--workspace-root`. `pnpm clean` and `pnpm ci` also clean root `node_modules` in workspaces without a root `package.json`. `pnpm clean --lockfile` now removes per-project lockfiles and lockfiles inside a custom `lockfileDir`.

- pnpm now reports `globalPackagesConfig` in the global `config.yaml` as a setting of a newer pnpm version. It no longer suggests a similarly named setting.

- Updated dependencies:
  - @pnpm/hooks.pnpmfile@1100.0.40
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
  - @pnpm/workspace.workspace-manifest-reader@1100.3.3
