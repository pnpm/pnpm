## 1104.2.2

### Patch Changes

- A warm `pnpm install` reuses on-disk package metadata for five minutes when the registry does not send an ETag. Registries that send an ETag, including the public npm registry, still revalidate with a conditional request. `pnpm update` still fetches current metadata [pnpm/pnpm#13976](https://github.com/pnpm/pnpm/issues/13976).

- With `minimumReleaseAge` set, re-resolving the lockfile no longer rewrites the `peerDependencies` recorded for a package whose version did not change. This happened when the registry metadata of a package differed from the `package.json` in its tarball [#13988](https://github.com/pnpm/pnpm/issues/13988).

- If an offline install fails because the registry metadata cache uses the layout from before pnpm 11.27 and 12.4, the error now names the older mirror on disk and explains that one online install repopulates the cache [#15656](https://github.com/pnpm/pnpm/issues/15656).

- `pnpm install --offline` and `pnpm add --offline` now resolve a version range to the newest matching version whose tarball is already in the store. They used to pick the newest version in the cached metadata and fail with `ERR_PNPM_NO_OFFLINE_TARBALL` when its tarball was missing [#10715](https://github.com/pnpm/pnpm/issues/10715).

- pnpm no longer revalidates cached registry metadata when the registry sends `Cache-Control: max-age=0`, `no-cache`, or `no-store`. It downloads the metadata again, so a version newly published to such a registry is visible on the next install [#13487](https://github.com/pnpm/pnpm/issues/13487).

- With `resolutionMode: time-based` and `minimumReleaseAge` both set, `pnpm install` no longer reports a subdependency as too new when only the time-based cutoff excludes it. Such subdependencies used to fail a strict install with `ERR_PNPM_NO_MATURE_MATCHING_VERSION`, or were added to `minimumReleaseAgeExclude` [#13569](https://github.com/pnpm/pnpm/issues/13569).

- With `resolutionMode: time-based`, a transitive dependency that has no matching version published before the time-based cutoff now resolves to the lowest matching version allowed by `minimumReleaseAge`. pnpm picks a version younger than `minimumReleaseAge` only if no older version matches [#16298](https://github.com/pnpm/pnpm/issues/16298).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/resolving.jsr-specifier-parser@1100.0.9
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.20
  - @pnpm/resolving.registry.types@1100.2.2
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/resolving.tarball-url@1101.1.3
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.index@1100.3.3
