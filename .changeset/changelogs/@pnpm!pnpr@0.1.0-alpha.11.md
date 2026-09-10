## 0.1.0-alpha.11

### Minor Changes

- `pnpm install` now resolves Cargo dependencies through the server configured in `pnprServer`. The client no longer fetches one sparse-index file per crate in the dependency graph. If the server does not serve Cargo resolution, pnpm resolves Cargo dependencies locally.

- `cargo search` works against a pnpr registry. It matches on crate name over the crates the registry hosts. Each result reports the crate's newest release that is not yanked. A crate whose releases are all yanked reports its newest release.

- pnpr can now publish packages of more than one ecosystem in a single transaction. `PUT /-/pnpr/v0/publish` takes a batch whose entries each name their `ecosystem`, so a workspace that ships an npm package, a crate and a Python distribution releases them together. A batch that fails a check publishes none of it. A server that stops midway finishes the release on the next startup. An entry without an `ecosystem` is an npm publish document, the same one `PUT /-/pnpm/v1/publish` takes.

- pnpr can now serve browser registry UIs through an origin allowlist. Search supports pagination and maintainer filters. Authenticated publishers are recorded for discovery. Organization package listings are available. Upstream discovery can be enabled per registry.

- Registries that serve more than one package ecosystem now address them through `/npm/`, `/cargo/`, and `/pypi/`. A registry that serves only one ecosystem continues to serve packages at the root.

- pnpr can resume OCI uploads across replicas when using S3 storage. Abandoned shared upload sessions expire at startup after 24 hours of inactivity.

  `pnpr oci-gc --registry <name>` collects old, unreferenced image blobs while registry writers are stopped. Use `--dry-run` to preview the cleanup.

- Added paginated browsing of hosted npm packages and Cargo crates through the search APIs with `browse=true`. Results respect registry routing and package access rules.

- pnpr can now serve Cargo and Python registries alongside npm from one instance. Each ecosystem has its own URL prefix, `/npm/`, `/cargo/`, or `/pypi/`. Existing npm URLs keep working.

  Hosted Cargo registries support `cargo publish`, `cargo yank`, and crate downloads. Hosted Python registries support `pip install --index-url` and `twine upload`. Upstream registries can proxy crates.io and PyPI with checksum-verified downloads. A router can combine sources from all three ecosystems.

- pnpr now reads every command-line option from an environment variable when the flag is omitted. The variable is named after the flag with a `PNPR_` prefix, so `--public-url` becomes `PNPR_PUBLIC_URL` and `--disable-resolver` becomes `PNPR_DISABLE_RESOLVER`. A flag given on the command line wins over its environment variable. Boolean flags accept `true`, `1`, `yes`, `on`, `false`, `0`, `no`, and `off`.

- OCI manifests can be published alongside packages through `PUT /-/pnpr/v0/publish`. Upload their layers before submitting the batch [pnpm/pnpm#14630](https://github.com/pnpm/pnpm/issues/14630).

- Authorized users can delete unreferenced OCI blobs. Blobs reachable from retained manifests remain protected during concurrent publishes [pnpm/pnpm#14630](https://github.com/pnpm/pnpm/issues/14630).

- pnpr now supports ranged OCI blob downloads. Cross-repository blob mounts reuse layers from readable repositories. The OCI referrers API lists artifacts attached to an image. Tag and repository listings support pagination with `n` and `last` [pnpm/pnpm#14630](https://github.com/pnpm/pnpm/issues/14630).

- pnpr can cache image pulls from Docker Hub and GHCR. Upstream authentication supports repository-scoped Bearer tokens. pnpr verifies cached images by digest [pnpm/pnpm#14630](https://github.com/pnpm/pnpm/issues/14630).

- Set `oci.bearerAuth: true` to use short-lived, repository-scoped credentials with container clients [pnpm/pnpm#14630](https://github.com/pnpm/pnpm/issues/14630).

- OCI blob and manifest size limits are configurable with `oci.maxBlobBytes` and `oci.maxManifestBytes` [pnpm/pnpm#14630](https://github.com/pnpm/pnpm/issues/14630).

- Added OIDC browser sign-in. Administrators can map provider subjects to registry users. GitHub Actions can publish npm packages without a persistent registry token. Workload publishing is restricted to configured packages.

- pnpr can store pipeline run reports with `pipeline.enabled`. Configure each workspace's `access` and `publish` permissions under `pipeline.workspaces`.

  `pnpm pipeline --report` and `--report-to` submit run summaries and events. pnpr provides authenticated listing and detail endpoints and a web viewer.

- Added a registry directory endpoint for discovering named registries, ecosystem endpoints, and routing order. The directory only includes registries visible to the current user.

  Registry names can now be reused across ecosystems by grouping configuration under `registries.npm`, `registries.cargo`, `registries.pypi`, or `registries.oci`. Router sources and defaults resolve within each ecosystem.

- `pnpm install` now resolves Python dependencies through the server configured in `pnprServer`. The client no longer downloads a wheel to find out what it requires. If the server does not serve Python resolution, pnpm resolves Python dependencies locally.

- pnpr now supports sharing Cargo compilation caches between CI and developers through sccache. Configure `artifacts.compilerCaches` to grant separate read and publication access. sccache can combine its local disk cache with pnpr's remote cache.

- pnpr now serves container images. Declare a registry with `ecosystem: oci`. Push to it with `docker`, `podman`, or `skopeo`. The distribution API answers at `/v2/` on the host root. An image keeps its own name, with no registry key in the path. Sign in with `docker login`, using a pnpr token as the password.

### Patch Changes

- A staged publish is now approved once, whichever replica of a shared registry the approval reaches. A second approval of the same stage answers 409. Rejecting a stage stops an approval that has not published it yet [#12199](https://github.com/pnpm/pnpm/issues/12199).

- pnpr now canonicalizes package names consistently for npm, Cargo, and Python registry operations.

- pnpm can install Cargo dependencies from the sparse registry configured by `cargo.indexUrl`. The generated `Cargo.lock` records that registry as the source of every crate.

- pnpr now stores an uploaded crate or Python distribution and records it in the registry document in one crash-safe step, as it already does for an npm publish. A server that stopped between the two steps left behind a file that nothing pointed at. The next startup now finishes the publish.

- A URL pnpr's npm surface does not serve now answers `404`. A URL it serves only for other methods answers `405`. A `/~<name>/` registry prefix must arrive with an unencoded `~`, on every ecosystem surface.

- Pipeline run records are now stored with the hosted packages. A run submitted through one replica is listed and served by every other. On a local storage root the records move from `pipeline-runs/v0` to `.pipeline-runs/v0`. Move an existing directory there to keep its runs [#12199](https://github.com/pnpm/pnpm/issues/12199).

- pnpr aborts proxied blob downloads when their contents fail integrity verification. Invalid blobs remain excluded from the cache.

- A publish that loses a write race now reports the error as `document_write_conflict` on every registry surface. The message names the package document [#14599](https://github.com/pnpm/pnpm/issues/14599).

- pnpr now resolves JSR packages without configuration. npm.jsr.io is a built-in public route, like the npm registry. The built-in routes are reachable over HTTPS only.

- Fixed failed registrations when multiple users sign up concurrently with a libsql backend and a configured user limit.

- pnpr records a URL's query exactly as it was written when it redacts the credentials in an error message. It used to re-encode the query, rewriting `?options=a%20b` as `?options=a+b`.

- pnpr now rejects a package name that carries `?`, `#`, `%`, whitespace, or a control character. Artifact filenames are held to the same rule. A percent-encoded delimiter let a request read an upstream package under a name the access rules had not checked.

- A publish whose file another writer had already stored under the same name now answers `409` instead of `201`. The version it described was left out of the registry document, so the publish did not happen. This can only occur where several pnpr instances share one object store.
