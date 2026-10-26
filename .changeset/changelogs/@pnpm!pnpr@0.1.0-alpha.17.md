## 0.1.0-alpha.17

This release changes the storage layout, removes old commands and flag aliases, and renames configuration keys to `camelCase`. It fixes two security issues and several failures with libsql and Turso auth databases.

### Minor Changes

- pnpr stores packages under readable paths. `@scope/pkg` is stored in `packages/@scope/pkg/`, an unscoped package in `packages/@/<name>/`, and a tarball as `blobs/<version>.tgz`.

  A store written by an earlier pnpr is not read. Publish its packages again into an empty store.

- pnpr serves Prometheus metrics at `/-/metrics` and `/metrics` only when the config has a `metrics` block. Before, it served them to anyone.

  Set `metrics.token` to require a bearer token from the scraper. Set `metrics.listen` to serve metrics on a separate address, such as one on a private network, and not on the main address.

- Added `DELETE /{name}`, `DELETE /@{scope}/{name}`, `DELETE /{name}@{version}`, `DELETE /@{scope}/{name}@{version}`, `DELETE /{name}/-/{filename}`, and `DELETE /@{scope}/{name}/-/{filename}` routes that do not require a trailing `/-rev/{rev}` segment. `PUT /-/user/{name}` and `GET /-/user/{name}` now accept bare usernames in addition to the legacy `org.couchdb.user:` prefix.

### Patch Changes

#### Changes that need action

- Removed `pnpr upgrade-storage` and `pnpr config-check`.

- Removed the `--packument-ttl-secs`, `--no-check-hashes`, `--min-age-secs`, and `--max-age-secs` CLI flag aliases and the `PNPR_PACKUMENT_TTL_SECS` environment variable. Use `--packument-ttl` or `PNPR_PACKUMENT_TTL` with a duration such as `5m` or `365d`.

- pnpr configuration keys are now `camelCase`, including `maxUsers`, `tokenEnv`, `maxAge`, `syncInterval`, and `backend.postgres`. pnpr fails to start on a key in the old spelling, such as `max_users`.

- pnpr now serves batch publishes at `PUT /-/pnpr/v0/publish` and `PUT /npm/-/pnpr/v0/publish`. It no longer serves `PUT /-/pnpm/v1/publish`, so `pnpm -r publish --batch` needs pnpm 11.28.9 or 12.16.0.

- Hosted storage namespaces now include the ecosystem (`default/<ecosystem>/` and `orgs/<org>/<ecosystem>/`), so hosted registries of different ecosystems can share an `org` and grouped registries default their `org` to their own name. Multi-component OCI repository names now map each `/`-separated component to its own directory segment.

- Team rosters, rule overrides, and upstream cache keys now always include the registry ecosystem (`<ecosystem>/<name>`). Moving a registry into an ecosystem group keeps its stored state and cache. Package tombstones now use the package directory layout. Internal storage and cache directories now use unversioned names (`.team-rosters`, `.rule-overrides`, `.scim-users`, `.pipeline-runs`, `.tombstones`, `.uploads`, `.journal`, `.staging`, `.artifacts`, `shared-artifacts`, `resolver-store`, and `resolver-cache`).

#### Security

- Fixed memory exposure and allocation handling vulnerabilities when processing zstd-compressed data [#17388](https://github.com/pnpm/pnpm/pull/17388).

- Signing in pnpm and pnpr no longer uses arithmetic that may leak the private key through timing. This covers shared artifacts, OCI registry tokens, and OIDC login state [advisory](https://rustsec.org/advisories/RUSTSEC-2026-0387).

#### Auth database

- Fixed pnpr occasionally crashing with a libsql or Turso auth database, most often on Windows. pnpr closed each database connection twice when it dropped one.

- pnpr no longer loses its libsql or Turso auth database after a request is cut off. Before, a client that disconnected during a token lookup could make every later login and authenticated request fail with a 500 until pnpr restarted. The new `backend.libsql.maxConnections` setting limits how many auth database operations run at once, and defaults to 10.

- pnpr now starts against a Turso auth database it has already set up. The second start failed with a unique-constraint error on the user counter. Registering a username that is already taken on Turso now gets the same answer as on a local database.

- pnpr now gives up on a TCP connection attempt to the libsql auth database after 10 seconds. At startup it retries an attempt that timed out until the startup deadline, so a dropped connection attempt no longer stops the server.

- The libsql auth backend's `replicaPath` setting now works with Turso Cloud, as it does with a self-hosted `sqld`. pnpr used to fail to start against Turso Cloud with an embedded replica configured [pnpm/tasks#183](https://github.com/pnpm/tasks/issues/183).

- pnpr now reuses its HTTP connections to a remote libsql or Turso auth database. Each token lookup opened a new connection before, and against Turso it paid a new TLS handshake.

- Token-authenticated requests are now handled without waiting for the token's last-used time to be written to the auth database. Every such request waited for that write, a database round trip on a shared or replicated database. The server now records a token's last use in the background, at most once a minute per token [pnpm/tasks#194](https://github.com/pnpm/tasks/issues/194).

#### Publishing, installing, and authentication

- OIDC workload credentials from GitHub Actions now verify. Discovery failed for issuers that publish no `authorization_endpoint`, so every such request answered 500 [#17300](https://github.com/pnpm/pnpm/issues/17300).

- Fixed OCI image pushes to pnpr when its S3 storage is Cloudflare R2. Every blob upload failed with a `blob_upload_conflict` error.

- Installs delegated to a pnpr server now resolve every prefix declared for a registry URL. A dependency or lockfile entry that used the second of two prefixes for the same registry, such as `zeta:` next to `work:` in `namedRegistries`, failed on the server [pnpm/tasks#204](https://github.com/pnpm/tasks/issues/204).

- Served `dist.tarball` URLs on `.tgz` registries now use the `<name>@<version>` route. The `/-/<filename>` route remains supported for existing lockfiles and older clients.

- Registry revisions are now addressed at `<registry>/<name>@<version>+r<revision>` [pnpm/tasks#208](https://github.com/pnpm/tasks/issues/208).

#### Output

- `pnpr config check` now names the configured auth backend, such as libsql, PostgreSQL, or MySQL. It printed `htpasswd` for every backend.

  `pnpr config check` now lists the default registry of every ecosystem. It listed only the npm default registry.

- The access log record under `pnpr::access` now prints each field once. The pretty format printed every field twice and `user` three times. The JSON format repeated the fields under `span` [pnpm/tasks#195](https://github.com/pnpm/tasks/issues/195).
