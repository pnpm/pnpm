## 1102.1.9

### Patch Changes

- `pnpm audit signatures` now verifies signatures against the integrity recorded in the lockfile. Packages without a recorded integrity cannot pass signature verification.

- The interactive `pnpm audit --fix` picker now shows each patched version with the `saveExact` and `savePrefix` style that the override is written with [pnpm/pnpm#13209](https://github.com/pnpm/pnpm/issues/13209).

- `pnpm licenses` now removes terminal control characters from package metadata in table output.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.32
  - @pnpm/config.reader@1102.3.4
  - @pnpm/config.version-policy@1100.2.7
  - @pnpm/config.writer@1100.0.32
  - @pnpm/constants@1102.0.1
  - @pnpm/deps.compliance.audit@1101.0.42
  - @pnpm/deps.compliance.license-scanner@1101.0.10
  - @pnpm/deps.compliance.sbom@1101.0.9
  - @pnpm/deps.security.signatures@1102.0.9
  - @pnpm/error@1100.2.2
  - @pnpm/installing.commands@1101.4.5
  - @pnpm/lockfile.fs@1100.2.13
  - @pnpm/lockfile.utils@1102.1.6
  - @pnpm/lockfile.walker@1100.0.27
  - @pnpm/network.auth-header@1101.1.16
  - @pnpm/network.fetch@1100.1.22
  - @pnpm/pkg-manifest.utils@1100.4.9
  - @pnpm/store.path@1100.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.3
