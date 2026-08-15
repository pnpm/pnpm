## 1101.6.2

### Patch Changes

- `pnpm audit --fix` and `pnpm audit --fix update` no longer add `minimumReleaseAgeExclude` entries for patched versions that were published before the `minimumReleaseAge` cutoff. The publish time of each minimum patched version is now checked against the registry metadata, and only versions young enough to be blocked by the age gate get an exclusion entry [#11563](https://github.com/pnpm/pnpm/issues/11563).

- `pnpm sbom` no longer emits components for optional platform-specific dependencies that cannot be installed on the current platform (for example, the native `@rolldown/binding-*` variants for other operating systems). Such packages are present in the lockfile but are never downloaded, so their license (and other metadata) could not be resolved and they appeared in the SBOM without one. `pnpm sbom --lockfile-only` still describes the whole lockfile graph, which is platform-independent by design.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.23
  - @pnpm/config.reader@1101.17.0
  - @pnpm/config.version-policy@1100.2.0
  - @pnpm/config.writer@1100.0.22
  - @pnpm/deps.compliance.audit@1101.0.31
  - @pnpm/deps.compliance.license-scanner@1100.1.2
  - @pnpm/deps.compliance.sbom@1100.4.1
  - @pnpm/deps.security.signatures@1101.3.1
  - @pnpm/error@1100.1.2
  - @pnpm/installing.commands@1100.15.0
  - @pnpm/lockfile.fs@1100.2.2
  - @pnpm/lockfile.utils@1101.1.0
  - @pnpm/network.auth-header@1101.1.10
  - @pnpm/network.fetch@1100.1.12
  - @pnpm/store.path@1100.0.5
  - @pnpm/workspace.project-manifest-reader@1100.0.24
