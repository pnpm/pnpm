## 0.1.0-alpha.22

### Minor Changes

- Removed platform variants. A package version has one archive again.

  pnpm no longer reads `publishConfig.variants` or the `variant` field of `package.json`. `pnpm pack --variant` and the `--upload-variant`, `--variant-integrity` and `--no-variants` options of `pnpm publish` are gone. pnpr no longer accepts variant archives with a version or serves them at `<name>@<version>/variants/<tag>`.

  To ship builds for several platforms, publish each as its own package and list them in `optionalDependencies`. A frozen install of a lockfile that records the variants of a registry package fails with `ERR_PNPM_BROKEN_LOCKFILE`. Run `pnpm install` to resolve the lockfile again.

### Patch Changes

- Fixed a shared build artifact publication failing with a broken pipe after pnpr refused one of its blob uploads before reading it.

- Cargo resolution now locks the path dependencies outside the workspace that a client sends.

- Fixed OCI referrer filters and registry paths that contain a literal plus sign. npm and Cargo searches now decode UTF-8 search terms correctly.

- `pnpm install` with `pnprServer` now rejects registry-keyed lockfile entries whose resolution uses a non-registry source with `ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`. The `pnpr` lockfile verification and resolution endpoints enforce the same check before accepting or caching a lockfile.

- pnpr now uses the caller's registry permissions when resolving packages from its own Cargo and PyPI indexes.

- OCI blob upload continuation requests on `/v2/<name>/blobs/uploads/<id>` now check current repository publish permissions and require the caller who started the upload session.

- `POST /-/npm/v1/security/advisories/bulk` now preserves explicit OSV versions alongside semver ranges, skips Git ranges, and applies `limit` bounds. It also derives severities from CVSS vectors and links non-GHSA advisories to valid URLs.

- Owner changes for Cargo, PyPI, and OCI now require an `annotate` grant. Transparency log reads honor `read` grants for the packages the registry claims, including for administrators.

- pnpr rejects Cargo and Python uploads with ambiguous archive paths. Python ZIP uploads now have the same 512 MiB decompressed size limit as compressed tar uploads.

- When the version tagged `latest` is removed, pnpr now restores the most recent former `latest` that is still published. If none remains, pnpr prefers the highest remaining stable release. Moving `latest` also updates the package README.

  pnpr rejects npm publish documents with no versions. Fetching package metadata with `?write=true` returns the current revision. Deprecation updates check that revision on every request, reject non-string messages, and clear the annotation for an empty message.

- pnpr rejects stale revisions on requests to unpublish a package or replace its maintainers. Hosted packages now support `pnpm owner add`, `pnpm owner rm`, `pnpm star`, `pnpm unstar`, and `pnpm stars`. `npm team ls` now displays team and member names.

- `pnpm install` with `pnprServer` configured now falls back to local resolution when the project declares `file:`, `link:`, or local path dependencies. `pnpr` also rejects local filesystem dependency specifiers on `/-/pnpr/v0/resolve`.

- Installs through a pnpr server now enforce `minimumReleaseAgeStrict` and add fallback versions to `minimumReleaseAgeExclude` when strict mode is off.

- Hosted registries serving tar archives now return the release record bytes covered by their transparency log. Version manifest requests now resolve dist-tags and exact upstream versions ending in `+r<N>`.

- The resolve endpoint now accepts `injectWorkspacePackages`, `dedupeInjectedDeps`, `ignoredOptionalDependencies`, `updateChecksums`, and a `dependenciesMeta` for each project.

- Restoring an earlier registry revision with `POST /-/pnpr/v0/admin/registry-revisions/{version}/restore` no longer creates default grants for registries that a later revision removed. A restored registry with no grant now serves nobody until an admin adds a grant for it.

- `pnpr cache-gc`, server startup, and background cache eviction now retain cached Cargo, Python, and OCI upstream files while their upstream registries remain configured. Before, orphan pruning deleted cached crate archives, Python wheels, source distributions, PEP 658 metadata, and OCI manifests and blobs even when their upstream registry was active.

- pnpr now returns freed memory to the operating system. On Linux, its resident memory grew to several GB while serving many large packuments at once and stayed there after the requests finished. pnpr also keeps more rendered packuments in its cache, so repeated reads of large packages need less memory and time [#18206](https://github.com/pnpm/pnpm/issues/18206).

- `POST /-/pnpr/v0/resolve` and `POST /-/pnpr/v0/verify-lockfile` now route tarballs from split-domain private upstream registries through their `/~<name>/` registry endpoints across resolved lockfiles and frozen prefetch frames. Previously, packuments that placed `dist.tarball` on a separate CDN host left raw CDN URLs and presigned query parameters in the returned lockfile.

- `POST /-/pnpr/v0/verify-lockfile` and `POST /-/pnpr/v0/resolve` now enforce caller read permissions for every private route recorded in a cached lockfile verdict. Updating grants through the admin API now invalidates cached resolutions and lockfile verdicts immediately. A running server applies a `pnpr grants import` within about 10 seconds.

- Router registries now partition npm security audit requests by package route, keeping hosted packages local and sending each upstream only the packages routed to it. The audit endpoints now accept `Content-Encoding: gzip` requests such as those sent by `npm audit`. `POST /-/npm/v1/security/audits` and `POST /-/npm/v1/security/audits/quick` now also evaluate advisories from the local OSV database when `--osv` is enabled.

- pnpr now fetches the package document when an upstream answers a version manifest request with HTTP 501. Scoped version manifests and attestations use an encoded package name.

  Package resolution and lockfile verification now send custom upstream headers on npm, Cargo, and Python package metadata requests. Importing a registry with credentials requires HTTPS or loopback HTTP, including IPv4-mapped loopback addresses.

- pnpr now honors `Cache-Control: no-cache` on upstream package metadata requests. A failed refresh for an unlisted version returns the upstream error and can be retried immediately.

- The pnpr binary is smaller. On macOS on Apple silicon, it is about 6 MB smaller.

- Approving a staged version now publishes its release notes. When different authenticated accounts stage and approve the version, pnpr records the approver and reports `stagedPublish` trust. Downloading a staged version with provenance returns the package tarball.
