## 1104.2.12

### Patch Changes

- `pnpm cat-file` can read executable files from the store. `pnpm cache list`, `view`, and `delete` recognize mixed-case package names.

  `pnpm clean` removes custom virtual stores with hidden directory names. In Plug'n'Play projects it also removes the generated loader on a full clean.

- `minimumReleaseAge` must now be a finite, non-negative number of minutes. Other values fail with `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE`. Before, such a value turned off the release age check or crashed the install.

- `pnpm outdated` now reports newer prereleases of an installed prerelease. It checks the dist-tag that points to the newest version of the installed prerelease channel, such as `next` for `3.0.0-rc.0`, alongside `latest` [#7339](https://github.com/pnpm/pnpm/issues/7339).

- `pnpm install --frozen-lockfile` now revalidates registry metadata when verifying a locked tarball whose version is missing from the cached package listing.

- Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/config.pick-registry-for-package@1101.0.7
  - @pnpm/config.version-policy@1100.2.12
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/crypto.hash@1100.0.10
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/fetching.types@1100.0.4
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/resolving.jsr-specifier-parser@1100.0.11
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.27
  - @pnpm/resolving.registry.types@1100.2.7
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/resolving.tarball-url@1101.1.8
  - @pnpm/store.cafs@1100.3.12
  - @pnpm/store.index@1101.0.5
  - @pnpm/types@1102.1.5
