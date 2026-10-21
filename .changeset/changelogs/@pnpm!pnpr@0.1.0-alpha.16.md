## 0.1.0-alpha.16

pnpr 0.1.0-alpha.16 serves every ecosystem under its own URL prefix, moves hosted storage to a new layout upgraded with `pnpr upgrade-storage`, and replaces `artifacts.compilerCaches` and `pipeline.workspaces` with `artifacts.orgs`. It also adds tar registries, zstd metadata and tarball compression, npm audit and provenance endpoints, Prometheus metrics, and `pnpr` maintenance commands.

### Minor Changes

- pnpr now serves npm under `/npm/` even when npm is the only ecosystem it serves. The npm registry URL is `https://<pnpr>/npm/`, and a named registry is at `https://<pnpr>/npm/~<name>/`.

  Cargo, Python, and container image registries likewise always sit under their own prefix. The default container image registry still answers at `/v2/` on the host root.

- pnpr stores hosted packages in a new layout. Each org has its own namespace, and every package keeps its document and blobs in separate places. The layout is the same on disk and in S3.

  pnpr no longer starts on a store in the earlier layout. Back the store up and convert it with `pnpr upgrade-storage`. On first start, pnpr also empties the proxy cache, which refills on demand.

- `artifacts.orgs` declares who may read and who may publish what an organization owns: its signed artifacts, its Cargo compilation cache, and its pipeline run records. It replaces `artifacts.compilerCaches` and `pipeline.workspaces`, which are no longer accepted.

  Signed artifacts no longer require the organization's name to match the account that reads or publishes them. Pipeline runs are recorded under an organization, and run URLs include it. Runs recorded by earlier versions are not listed.

- All configuration sections now strictly validate schema fields and reject unrecognized keys, unset environment variable references in secret fields, and the retired top-level `uplinks` and `upstreams` blocks. Configure upstreams in `registries` instead. Configuration keys accept both camelCase and snake_case names. Per-ecosystem validation rejects `teamsManagedBy: api` and package `unpublish` rules on Cargo and PyPI registries.

  `secrets` now supports independent cache and OCI secret configuration, and derives domain-separated subkeys when a single root `secret` is provided. Moving a registry into an ecosystem group preserves its team rosters and rule overrides, persisted documents now record a `formatVersion`, and the default configuration file proxies the public npm registry with self-registration disabled by default.

- A registry with `tarballs: tar` serves each version as the uncompressed tar of its `.tgz`. pnpr compresses the tar with zstd or gzip, as the request accepts. The registry's metadata pins the hash of the tar.

  Every registry now also serves a version's tarball at `<registry>/<name>@<version>`.

- The `/npm/-/tarballs/sha512/<digest>` route now names a package by the SHA-512 of its uncompressed tar, on `.tgz` and tar registries alike. It serves the tar with `gzip` or `zstd` content coding, or none, as the client's `Accept-Encoding` allows. pnpr records a hosted version's digest reference when the version is published. pnpr checks a proxied upstream revision against its digest before it caches or serves it.

  pnpr now keeps `dist.revision: 0` for an original in upstream revision metadata. pnpr keeps revision metadata only when its history lists revision 0. A revision's `dist` omits `shasum` and `signatures`. Tar registries keep revision metadata.

- pnpr now serves verified tarball metadata under `dist.pnpr` in package metadata once it has scanned a version's tarball: `requiresBuild`, precalculated binary targets and shebang runtimes, unpacked size, and file count. `POST /-/pnpr/v0/resolve` also includes `requiresBuild` on `package` frames. pnpr strips any `dist.pnpr` field sent by a publisher or an upstream registry.

- `PUT /-/pnpr/v0/artifacts/blob` stores one blob of a signed artifact ahead of the envelope that names it, streamed from the request body. A blob can be up to 4 GiB. `HEAD` on the same URL tells whether a blob is stored.

  `artifacts.quota.ownerGiB` and `artifacts.quota.totalGiB` set how much the artifact store keeps. They default to 1 GiB per owner and 10 GiB in total.

- pnpr now supports token creation via `POST /-/npm/v1/tokens` with `--read-only`, CIDR restrictions, expiry, and token naming. Expired tokens are rejected during authentication. Token authentication updates the `last_used_at` timestamp. Read-only tokens are permitted to access npm audit endpoints [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- pnpr now supports the npm audit endpoints: `POST /-/npm/v1/security/advisories/bulk`, `POST /-/npm/v1/security/audits`, and `POST /-/npm/v1/security/audits/quick`. Bulk advisory queries evaluate against local records when an OSV database is enabled. Requests forward to configured upstream registries when OSV is disabled [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- pnpr now accepts `.sigstore` provenance attachments during `npm publish` and serves `/-/npm/v1/attestations/{spec}` and `/-/npm/v1/keys` endpoints [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- pnpr now supports cache size caps, TTL retention, and orphan namespace pruning. The `cache:` setting accepts `maxSize` and `maxAge` limits to bound upstream proxy storage and evict stale files. Startup sweeps prune orphaned upstream namespaces left behind by ephemeral per-process secrets. The new `pnpr cache-gc` command reclaims cache storage on demand with support for dry runs, custom size caps, age limits, and orphan-only cleanup [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- pnpr now exposes Prometheus metrics at `GET /metrics` and `GET /-/metrics`, tracks request IDs, and enriches access logs [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131). The metrics endpoint emits standard Prometheus text format with in-flight gauges, request counters, duration histograms, and cache statistics. Requests forward and echo `x-request-id` headers or receive generated UUIDs. Access logs under `pnpr::access` record request IDs, authenticated usernames, client IPs, user agents, status codes, latencies, and byte counts.

- pnpr now emits structured audit records under tracing target `pnpr::audit` at `info` level [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131). Audit events cover user management, rule overrides, team membership and package access, SCIM operations, and package deletion. When structured JSON logging is enabled, each audit record inherits the active request metadata including the request ID and client IP.

- pnpr now resolves client IP addresses through trusted reverse proxies using the new `trustedProxies` and `trustedProxyHeader` settings. Forwarding headers from trusted peers are traversed to record the client IP in access logs and enforce token CIDR restrictions [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- Package write requests now check permissions before buffering large request bodies. Upstream packument fetches are coalesced across concurrent requests. Login endpoints enforce rate limits, and incoming requests support a configurable `requestTimeout` [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- pnpr now responds to Unix `SIGTERM` signals with graceful shutdown, stopping new connections and completing in-flight requests before exiting. Dedicated readiness at `/-/ready` and liveness at `/-/live` are now available for container orchestration, and `/-/ping` returns 503 when backend storage or auth is inaccessible [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

### Patch Changes

- Self-registration can no longer claim the username of a configured admin, an OIDC binding, a workload binding, or a SCIM-provisioned user. The new `pnpr add-user` command creates accounts under reserved usernames [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- New auth files default to 0600 permissions on Unix. Login requests on disabled registration return 401 to prevent user enumeration. Passwords exceeding 71 bytes are rejected to prevent bcrypt truncation. Published package versions remain immutable across partial unpublish requests.

- pnpr now records package maintainers on publish, supports the `$owners` access token in package access rules, and handles `npm owner` maintainer mutations. Unpublished packages and versions are recorded in tombstones to prevent package squatting and republishing of removed versions [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- pnpr can now use a libsql or Turso database for users and tokens. Before, a `backend.libsql` block made pnpr crash at startup. pnpr checks the database's certificate against the system trust store. It refuses to send `authToken` over plain `http://` unless the database is on a loopback host.

- pnpr now tracks database schema versions and migrates existing SQLite, PostgreSQL, MySQL, and libsql auth databases at startup. MySQL auth backends now use binary collation for case-sensitive username lookups [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).

- Added the login web flow at `/-/v1/login` so `npm login` and `pnpm login` authenticate via browser SSO when an OIDC login provider is configured.

- Added the `pnpr serve` subcommand. Bare `pnpr` remains supported as an alias. Server flags such as `--listen` and `--packument-ttl` are now scoped to `serve` and are no longer accepted on maintenance commands.

- Added maintenance subcommands to `pnpr`. `config check` inspects configuration diagnostics, `verify` audits hosted documents and storage blobs, `export` dumps packages and authentication records, `migrate-storage` migrates packages from local storage to S3, and `import-htpasswd` imports Apache and Verdaccio user accounts with automatic bcrypt upgrade.

- pnpr now compresses package metadata with zstd for clients that accept it. A full packument such as vite's is served in about 650 KB with zstd and 4.9 MB with gzip. pnpr also accepts zstd-compressed responses from upstream registries.

- Packument responses now emit `ETag`, `Vary: Accept`, and `Cache-Control` headers. Conditional requests sending `If-None-Match` or `If-Modified-Since` now return `304 Not Modified` when unchanged.

- When pnpm sends the `addMissingPeerTypes` setting, pnpr now resolves the matching `@types` packages as optional peers of the packages that peer-depend on a library.

- Added protocol versioning between the pnpm client and pnpr server, verified applied resolution policies in resolution responses, and preserved structured error codes across the wire.

- Added a Node.js fallback wrapper script for `@pnpm/pnpr` when install scripts are skipped, updated Node engines requirement to `>=18`, and added an init process to the pnpr Docker container image.
