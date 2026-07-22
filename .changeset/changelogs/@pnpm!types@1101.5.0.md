## 1101.5.0

### Minor Changes

- Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.

- Added `update` and `audit` settings sections to `pnpm-workspace.yaml`, superseding the awkwardly named `updateConfig`, `auditConfig`, and top-level `auditLevel` settings:

  ```yaml
  update:
    ignoreDeps: # was updateConfig.ignoreDependencies
      - webpack
      - "@babel/*"

  audit:
    level: high # was auditLevel
    ignore: # was auditConfig.ignoreGhsas
      - GHSA-xxxx-yyyy-zzzz
  ```

  `update.ignoreDeps` lists dependency name patterns that `pnpm update` and `pnpm outdated` should skip. `audit.level` and `audit.ignore` tune `pnpm audit`.

  The deprecated `updateConfig`, `auditConfig`, and `auditLevel` settings keep working until the next major version. When both a new section value and its deprecated counterpart are set, the new section takes precedence and a warning is printed. Both the TypeScript CLI and the Rust config surface (pacquet) recognize the new sections.
