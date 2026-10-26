## 1102.1.12

### Patch Changes

- `pnpm audit --fix` and `pnpm audit --interactive` now fail with `ERR_PNPM_AUDIT_FIX_WITH_IGNORE` when combined with `--ignore` or `--ignore-unfixable`. Before, the fix ran and the ignore flags had no effect.

- `pnpm audit --ignore` and `pnpm audit --ignore-unfixable` now exit with a non-zero code when vulnerabilities at or above the audit level remain after ignoring. Before, these commands always exited with code 0.

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.4
  - @pnpm/cli.utils@1101.0.34
  - @pnpm/config.pick-registry-for-package@1101.0.4
  - @pnpm/config.reader@1102.3.7
  - @pnpm/config.version-policy@1100.2.10
  - @pnpm/config.writer@1100.0.35
  - @pnpm/deps.compliance.audit@1101.0.44
  - @pnpm/deps.compliance.license-scanner@1101.0.13
  - @pnpm/deps.compliance.sbom@1101.0.12
  - @pnpm/deps.security.signatures@1102.0.11
  - @pnpm/installing.commands@1101.4.8
  - @pnpm/lockfile.fs@1100.2.15
  - @pnpm/lockfile.types@1100.1.6
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/lockfile.walker@1100.0.29
  - @pnpm/network.auth-header@1101.1.18
  - @pnpm/network.fetch@1100.1.24
  - @pnpm/pkg-manifest.utils@1100.4.11
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
