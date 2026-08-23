## 1100.1.37

### Patch Changes

- `pnpm init` now pins the exact pnpm version instead of a `^` range, and records it in the `packageManager` field alongside `devEngines.packageManager`. Corepack reads only `packageManager` and accepts nothing but an exact version, so it rejected the generated `package.json` with "expected a semver version" [pnpm/pnpm#13969](https://github.com/pnpm/pnpm/issues/13969). A package created inside an existing workspace is still left unpinned — it follows the pin at the workspace root — and `--no-init-package-manager` still scaffolds a manifest without any pin. In pnpm 12, `pnpm init` also honors `initType` and its `--init-type` flag, so the manifest it writes is the same one pnpm 11 writes.

- Updated dependencies:
  - @pnpm/cli.meta@1100.0.15
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.reader@1102.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-writer@1100.0.15
