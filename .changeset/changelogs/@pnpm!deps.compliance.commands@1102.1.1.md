## 1102.1.1

### Patch Changes

- Fixed `pnpm audit --fix` failing with `ERR_PNPM_INVALID_FIX_OPTION` when used without a value, including when another flag follows it, as in `pnpm audit --fix --json` [#13261](https://github.com/pnpm/pnpm/issues/13261). Fixed `pnpm audit --fix=override` ignoring the `saveExact` and `savePrefix` settings when writing vulnerability overrides [#11523](https://github.com/pnpm/pnpm/issues/11523).

- `pnpm audit` no longer counts advisories suppressed by `auditConfig` in its summary. The headline total and the `Severity:` breakdown now count only the advisories that survive the `ignoreGhsas` filter. Suppressed advisories get their own line, `2 ignored: 1 moderate | 1 critical`. A run whose advisories were all suppressed printed a red `1 vulnerabilities found` next to a zero exit code. It now reads `All found vulnerabilities were already reviewed and decided to be ignored` [#14535](https://github.com/pnpm/pnpm/issues/14535).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.26
  - @pnpm/config.reader@1102.1.1
  - @pnpm/config.version-policy@1100.2.3
  - @pnpm/config.writer@1100.0.25
  - @pnpm/deps.compliance.audit@1101.0.35
  - @pnpm/deps.compliance.license-scanner@1101.0.3
  - @pnpm/deps.compliance.sbom@1101.0.2
  - @pnpm/deps.security.signatures@1102.0.2
  - @pnpm/error@1100.1.4
  - @pnpm/installing.commands@1101.2.0
  - @pnpm/lockfile.fs@1100.2.6
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/lockfile.utils@1102.1.1
  - @pnpm/lockfile.walker@1100.0.22
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/network.fetch@1100.1.15
  - @pnpm/pkg-manifest.utils@1100.4.3
  - @pnpm/store.path@1100.0.7
  - @pnpm/workspace.project-manifest-reader@1100.0.27
