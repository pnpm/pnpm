## 1104.2.7

### Patch Changes

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- With `minimumReleaseAge` set and full metadata in use, pnpm now keeps using cached metadata that the registry confirms is current, also when that metadata has no publish times. Previously, pnpm downloaded such metadata again on every install.

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- `pnpm add` now reads the package name correctly from a tarball URL under a registry URL that has a path, such as `https://example.com/npm/`. Before, the registry path became part of the name, so a scoped package failed with `ERR_PNPM_INVALID_PACKAGE_NAME`.

- pnpm now fails with `ERR_PNPM_REGISTRY_NAME_MISMATCH` if a registry returns metadata for a package other than the one requested. Before, pnpm installed the package under the name in the metadata, so `allowBuilds` rules for the requested package did not apply to it.

- With `minimumReleaseAge` set, packages listed in `minimumReleaseAgeExclude` now resolve from cached metadata without a registry request when the lockfile already has the version their range picks. Packages resolve the same way without `minimumReleaseAge`.

- With `minimumReleaseAge` set, pnpm sends one conditional metadata request instead of two for a package whose cached metadata already holds the full document, but without every version's publish time.

- With `minimumReleaseAge` set, pnpm now revalidates a package's cached full metadata with its ETag, so an unchanged registry answers with `304`. Before, if the registry did not list a publish time for every version, pnpm downloaded that metadata again whenever the cached copy was older than `minimumReleaseAge`.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.pick-registry-for-package@1101.0.3
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.path@1101.0.7
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.24
  - @pnpm/resolving.registry.types@1100.2.4
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/resolving.tarball-url@1101.1.5
  - @pnpm/store.cafs@1100.3.8
  - @pnpm/store.index@1101.0.3
  - @pnpm/types@1102.1.2
