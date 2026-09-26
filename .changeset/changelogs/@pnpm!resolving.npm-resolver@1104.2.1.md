## 1104.2.1

### Patch Changes

- `pnpm audit --fix=update` now fixes vulnerabilities in dependencies declared through an npm alias. A specifier such as `"foo": "npm:vulnerable-pkg@1.0.0"` moves to the patched version and keeps the alias. Versions pinned with a leading `=` are fixed as well [#15155](https://github.com/pnpm/pnpm/issues/15155).

- `pnpm add` now saves the requested exact version when adding a dependency, even when the manifest already contains a version range [#6040](https://github.com/pnpm/pnpm/issues/6040).

- pnpm now keeps the configured default registry when `_auth` holds credentials for several registries and some of those registries serve package scopes.

  Lockfile verification checks a tarball hosted on a scoped registry against that registry's metadata, unless the package's own scope has a registry assigned [pnpm/pnpm#15530](https://github.com/pnpm/pnpm/issues/15530).

- A `runtime:` version range that contains `||` or a space, such as a `devEngines.runtime` version of `^22.18.0 || ^24.0.0`, now installs the requested runtime. pnpm used to install the npm package with the same name, such as `node` [#14817](https://github.com/pnpm/pnpm/issues/14817).

- Resolution no longer logs an error when a package metadata request fails and resolution succeeds via cached metadata [pnpm/pnpm#2522](https://github.com/pnpm/pnpm/issues/2522).

- When the registry stops sending data for longer than `fetchTimeout`, pnpm now reports that the metadata or tarball request timed out. Previously the error did not mention the timeout [#3646](https://github.com/pnpm/pnpm/issues/3646).

- With `trustPolicy: no-downgrade`, pnpm now resolves the newest matching version that is not a trust downgrade. Previously a dependency failed with `ERR_PNPM_TRUST_DOWNGRADE` even when an older version satisfied its range. `pnpm self-update` picks its target version the same way. A request for an exact version still fails [#14176](https://github.com/pnpm/pnpm/issues/14176).

- `pnpm install` and `pnpm update` now resolve a dependency range to the newest matching version that is not deprecated. A version already recorded in the lockfile is still used [#15128](https://github.com/pnpm/pnpm/issues/15128).

- Support SemVer build metadata in workspace dependency resolution

- A `workspace:` dependency now resolves to a workspace project whose version is not valid semver, such as `1` or `1.0`. `workspace:*`, `workspace:^`, and `workspace:~` match it. A range identical to the version also matches it [#4567](https://github.com/pnpm/pnpm/issues/4567).

- Fixed workspace packages with SemVer build metadata being skipped when they match the requested range and have the same version precedence as the registry package [#2812](https://github.com/pnpm/pnpm/issues/2812).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.pick-registry-for-package@1101.0.2
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/resolving.jsr-specifier-parser@1100.0.8
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.19
  - @pnpm/resolving.registry.types@1100.2.1
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/resolving.tarball-url@1101.1.2
  - @pnpm/store.cafs@1100.3.4
  - @pnpm/store.index@1100.3.2
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.range-resolver@1100.0.4
