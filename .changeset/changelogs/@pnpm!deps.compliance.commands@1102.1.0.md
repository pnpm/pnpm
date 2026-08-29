## 1102.1.0

### Minor Changes

- Added the `audit.ignorePrune` setting. When set to `true`, `pnpm audit --fix` removes ignored GHSA entries that no longer appear in the audit report.

### Patch Changes

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.0
  - @pnpm/cli.utils@1101.0.25
  - @pnpm/config.pick-registry-for-package@1101.0.1
  - @pnpm/config.reader@1102.1.0
  - @pnpm/config.version-policy@1100.2.2
  - @pnpm/config.writer@1100.0.24
  - @pnpm/deps.compliance.audit@1101.0.34
  - @pnpm/deps.compliance.license-scanner@1101.0.2
  - @pnpm/deps.compliance.sbom@1101.0.1
  - @pnpm/deps.security.signatures@1102.0.1
  - @pnpm/installing.commands@1101.1.0
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/lockfile.walker@1100.0.21
  - @pnpm/network.auth-header@1101.1.12
  - @pnpm/network.fetch@1100.1.14
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.project-manifest-reader@1100.0.26
