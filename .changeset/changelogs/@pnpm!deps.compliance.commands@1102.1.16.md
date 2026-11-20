## 1102.1.16

### Patch Changes

- `pnpm audit` now reports advisories whose `id` is a string. Such advisories were dropped, and `pnpm audit` reported no known vulnerabilities [#18184](https://github.com/pnpm/pnpm/issues/18184).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.5
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.pick-registry-for-package@1101.0.6
  - @pnpm/config.reader@1102.3.10
  - @pnpm/config.version-policy@1100.2.11
  - @pnpm/config.writer@1100.0.38
  - @pnpm/deps.compliance.audit@1101.0.48
  - @pnpm/deps.compliance.license-scanner@1101.0.17
  - @pnpm/deps.compliance.sbom@1101.0.16
  - @pnpm/deps.security.signatures@1102.0.13
  - @pnpm/installing.commands@1101.4.12
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/lockfile.walker@1100.0.31
  - @pnpm/network.auth-header@1101.1.19
  - @pnpm/network.fetch@1100.1.26
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
