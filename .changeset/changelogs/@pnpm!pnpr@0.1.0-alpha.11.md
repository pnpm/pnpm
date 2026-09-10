## 0.1.0-alpha.11

pnpr now serves Cargo, Python, and container registries alongside npm, publishes across all of them in one transaction, and signs users in through OIDC.

### Minor Changes

#### Cargo and Python registries

- pnpr now serves Cargo and Python registries alongside npm from one instance. Hosted Cargo registries support `cargo publish`, `cargo yank`, and crate downloads. Hosted Python registries support `pip install --index-url` and `twine upload`. Upstream registries can proxy crates.io and PyPI with checksum verified downloads, and a router can combine sources from all three ecosystems.

  A registry that serves more than one ecosystem addresses them through `/npm/`, `/cargo/`, and `/pypi/`. A registry that serves only one ecosystem keeps serving packages at the root, so existing npm URLs still work. A `/~<name>/` registry prefix must arrive with an unencoded `~`.

  Registry names can be reused across ecosystems by grouping configuration under `registries.npm`, `registries.cargo`, `registries.pypi`, or `registries.oci`. Router sources and defaults resolve within their own ecosystem.

- `pnpm install` now resolves Cargo and Python dependencies through the server configured in `pnprServer`. The client no longer fetches one sparse-index file per crate in the dependency graph, and no longer downloads a wheel to find out what it requires. If the server does not serve that resolution, pnpm resolves those dependencies locally.

- `pnpm install` can install Cargo dependencies from the sparse registry configured by `cargo.indexUrl`. The generated `Cargo.lock` records that registry as the source of every crate.

- `cargo search` works against a pnpr registry. It matches on crate name over the crates the registry hosts. Each result reports the crate's newest release that is not yanked, and a crate whose releases are all yanked reports its newest release.

#### Container images

- pnpr now serves container images [#14630](https://github.com/pnpm/pnpm/issues/14630). Declare a registry with `ecosystem: oci`, then push to it with `docker`, `podman`, or `skopeo`. The distribution API answers at `/v2/` on the host root, and an image keeps its own name with no registry key in the path. Sign in with `docker login`, using a pnpr token as the password, or set `oci.bearerAuth: true` to hand clients short lived, repository scoped credentials instead.

  Ranged blob downloads, cross repository blob mounts, the referrers API, and paginated tag and repository listings are supported. Authorized users can delete unreferenced blobs, and a blob that a retained manifest still reaches stays protected while another publish is running. `oci.maxBlobBytes` and `oci.maxManifestBytes` cap what a client may upload.

- pnpr can cache image pulls from Docker Hub and GHCR [#14630](https://github.com/pnpm/pnpm/issues/14630). Upstream authentication supports repository scoped bearer tokens, and pnpr verifies every cached image by digest. A proxied download whose contents fail verification is aborted and stays out of the cache.

- pnpr can resume OCI uploads across replicas when it keeps blobs in S3. Abandoned shared upload sessions expire at startup after 24 hours of inactivity. `pnpr oci-gc --registry <name>` collects old, unreferenced image blobs while registry writers are stopped, and `--dry-run` previews the cleanup.

#### Publishing and authentication

- pnpr can now publish packages of more than one ecosystem in a single transaction. `PUT /-/pnpr/v0/publish` takes a batch whose entries each name their `ecosystem`, so a workspace that ships an npm package, a crate, and a Python distribution releases them together. OCI manifests can ride along in the same batch once their layers are uploaded [#14630](https://github.com/pnpm/pnpm/issues/14630). A batch that fails a check publishes none of it, and a server that stops midway finishes the release on the next startup. An entry without an `ecosystem` is an npm publish document, the same one `PUT /-/pnpm/v1/publish` takes.

- pnpr now supports OIDC browser sign in. Administrators can map provider subjects to registry users, and GitHub Actions can publish npm packages without a persistent registry token. Workload publishing is restricted to the packages you configure.

#### Builds and pipelines

- pnpr can share Cargo compilation caches between CI and developers through sccache. Configure `artifacts.compilerCaches` to grant read and publication access separately. sccache can combine its local disk cache with pnpr's remote cache.

- pnpr can store `pnpm pipeline` run reports. Turn it on with `pipeline.enabled` and configure each workspace's `access` and `publish` permissions under `pipeline.workspaces`. `pnpm pipeline --report` and `--report-to` submit run summaries and events, and pnpr answers with authenticated listing and detail endpoints and a web viewer. Records are kept with the hosted packages, so a run submitted through one replica is listed and served by every other.

#### Discovery and administration

- pnpr can now serve browser registry UIs through an origin allowlist. Search supports pagination and maintainer filters, organization package listings are available, and authenticated publishers are recorded for discovery. Upstream discovery can be enabled per registry. Passing `browse=true` to the search APIs pages through the npm packages and Cargo crates a registry hosts, respecting its routing and package access rules.

  A registry directory endpoint lists the named registries, ecosystem endpoints, and routing order that the current user can see.

- pnpr now reads every command line option from an environment variable when the flag is omitted. The variable is named after the flag with a `PNPR_` prefix, so `--public-url` becomes `PNPR_PUBLIC_URL` and `--disable-resolver` becomes `PNPR_DISABLE_RESOLVER`. A flag given on the command line wins over its environment variable. Boolean flags accept `true`, `1`, `yes`, `on`, `false`, `0`, `no`, and `off`.

### Patch Changes

- pnpr now rejects a package name that carries `?`, `#`, `%`, whitespace, or a control character, and holds artifact filenames to the same rule. A percent encoded delimiter let a request read an upstream package under a name the access rules had not checked.

- pnpr now resolves JSR packages without configuration. npm.jsr.io is a built-in public route, like the npm registry. The built-in routes are reachable over HTTPS only.

- A publish that loses a write race now reports `document_write_conflict` on every registry surface, and the message names the package document [#14599](https://github.com/pnpm/pnpm/issues/14599). A publish whose file another writer had already stored under the same name now answers `409` instead of `201`, which it answered while leaving the version out of the registry document. Both cases arise only where several pnpr instances share one object store.

- A staged publish is now approved once, whichever replica of a shared registry the approval reaches [#12199](https://github.com/pnpm/pnpm/issues/12199). A second approval of the same stage answers `409`. Rejecting a stage stops an approval that has not published it yet.

- Registration no longer fails when several users sign up at the same time against a libsql backend with a configured user limit.

- pnpr now records a URL's query exactly as it was written when it redacts the credentials in an error message. It re-encoded the query, rewriting `?options=a%20b` as `?options=a+b`.
