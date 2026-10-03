## 1102.3.3

### Patch Changes

- pnpm now reports an `INVALID_SETTING` error when `allowUnusedPatches` in `pnpm-workspace.yaml` is not a boolean. A quoted value such as `"false"` was treated as `true`.

- pnpm now reports an `INVALID_SETTING` error when `ignoredOptionalDependencies` or `requiredScripts` in `pnpm-workspace.yaml` is not an array of strings.

- Updated dependencies:
  - @pnpm/config.matcher@1100.0.3
  - @pnpm/exec.esm-node-path-loader@1100.0.1
  - @pnpm/hooks.pnpmfile@1100.0.35
  - @pnpm/pkg-manifest.utils@1100.4.8
  - @pnpm/workspace.project-manifest-reader@1100.1.2
