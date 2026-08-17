## 1100.4.1

### Patch Changes

- An `integrity` recorded on a git dependency's resolution (`resolution: {type: git, repo, commit, integrity: sha512-…}`) is no longer treated as a checksum. pnpm never verifies a git checkout against such a hash — the commit pins the content — so it is now dropped when the lockfile is rewritten, and `pnpm sbom` no longer republishes it as a CycloneDX/SPDX checksum. Lockfiles carrying one also load again instead of failing with `ERR_PNPM_BROKEN_LOCKFILE` [#13042](https://github.com/pnpm/pnpm/issues/13042).

  `pnpm sbom` now also publishes the checksum of a `type: binary` runtime archive, which pnpm does verify.

- `pnpm sbom` no longer emits components for optional platform-specific dependencies that cannot be installed on the current platform (for example, the native `@rolldown/binding-*` variants for other operating systems). Such packages are present in the lockfile but are never downloaded, so their license (and other metadata) could not be resolved and they appeared in the SBOM without one. `pnpm sbom --lockfile-only` still describes the whole lockfile graph, which is platform-independent by design.

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.1.3
  - @pnpm/error@1100.1.2
  - @pnpm/lockfile.utils@1101.1.0
  - @pnpm/pkg-manifest.reader@1100.0.16
  - @pnpm/store.index@1100.2.4
  - @pnpm/store.pkg-finder@1100.0.29
