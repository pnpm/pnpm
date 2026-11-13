## 0.1.0-alpha.20

### Minor Changes

- Added `POST /-/pnpr/v0/admin/users` to create accounts through the admin API. `PUT /-/pnpr/v0/admin/users/{id}` no longer creates accounts. It replaces an existing account's password by account ID and returns `404` for unknown IDs [zkochan/pnpm-tasks#82](https://github.com/zkochan/pnpm-tasks/issues/82).

- pnpr audit records now carry `actor_id`, the account ID of whoever performed the action, which stays the same when the account is renamed. Sign-ins, failed logins, workload credential use, token creation and revocation, and publishes are now audited as `auth.sign_in`, `auth.login_failed`, `auth.workload_use`, `token.create`, `token.revoke`, and `package.publish`. Version and publication note events now name the actor by account name, and every event about another account names it with both `user` and `user_id`.

- Added `cloudflareCdn` to purge cached public packages after publishes, unpublishes, deprecations, and access-rule changes. The Cloudflare cache example serves anonymous cache hits without contacting the registry and limits cached responses to five minutes. [Issue](https://github.com/zkochan/pnpm-tasks/issues/56).

- Renamed and regrouped pnpr configuration keys:

  - An upstream registry's `maxAge` is now `packumentTtl`, the same name as the global setting it overrides.
  - Byte limits take size strings such as `4MiB`.
  - `oci.maxBlobBytes`, `oci.maxManifestBytes`, and `oci.chunkMaxBytes` are now `oci.maxBlobSize`, `oci.maxManifestSize`, and `oci.maxChunkSize`.
  - `artifacts.quota.ownerGiB` and `artifacts.quota.totalGiB` are now `artifacts.quota.perOwner` and `artifacts.quota.total`.
  - `secrets.cache` and `secrets.oci` are now `secret.cache` and `secret.oci`. `secret` takes either one string or that table.
  - `trustedProxies` is now a table. The proxy list moves to `trustedProxies.networks`.
  - `trustedProxyHeader` is now `trustedProxies.header`.
  - `log.type` is gone. pnpr always logs to stdout.
  - New `listen` and `publicUrl` keys set the bind address and the public URL. `--listen` and `--public-url` override them.

  pnpr now derives the private cache key from `secret`. Private cache entries written by an earlier version are no longer used, and `pnpr cache gc` removes them.

  An unset environment variable in `database.url` now fails startup, as it already did in the other credential fields. A `secret` that YAML reads as a number must be quoted. Config errors now give the line and column without quoting the surrounding lines of the file.

  An unparsable `routes.public` registry URL or package glob now fails startup. Before, pnpr logged a warning and ignored the rule.

  pnpr logs a warning when it listens on a non-loopback address without `publicUrl`. `--storage` no longer replaces a `cache` path set in the config.

- Tokens from a password login, and tokens from `npm token create` without an expiry, now expire after 90 days. The `auth.tokens.defaultLifetime` setting changes this lifetime.

  Added the `auth.tokens.maxLifetime` setting. It limits how long any token from a password login or `npm token create` stays valid, for example `90d`.

- A `packages:` entry without a `publish` list now admits only the package's owners and `auth.admins`. The account that first publishes a package, crate, Python project, or image repository becomes its owner. An npm package's owners are its maintainers. Before, any signed-in account could publish a new version of another account's package and move its dist-tags. Set `publish: $authenticated` to keep the old behavior.

- `cargo owner` now lists, adds, and removes the owners of a hosted crate. Python projects and image repositories take the same requests at `/pypi/owners/<project>` and `/oci/owners/<repository>`. A package's owners and `auth.admins` can change its owners.

- pnpr now keeps its registries in the metadata database instead of the config file. Every replica serves the same registries, and admins listed in the new `auth.registryAdmins` change them through `/-/pnpr/v0/admin/registries` without a config deploy. Each change is a revision that can be restored.

  A config file that declares `registries`, `defaultRegistry`, or `routes.public` is refused at startup. Move those blocks into a file of their own and store it with `pnpr registries import`. `pnpr registries export` prints the stored registries in that format. `pnpr registries import --check` fails when the stored registries differ from a file.

  On its first start against an empty database, pnpr stores a default set of registries: a proxy of the public npm registry and a hosted registry for private packages.

  An upstream credential must be a `${NAME}` reference to an environment variable that the config lists in `registryEnv`. An upstream URL with a user name, password, or query string is refused.

  `teamsManagedBy` and `rulesManagedBy` are removed. The team endpoints, `npm access grant`, and the rules endpoints now edit the stored registries. Teams and rule changes stored under the old settings are not carried over.

- A pnpr registry name must now start with a letter and contain only letters, digits, `.`, `_`, and `-`. It cannot be a reserved pnpm prefix name such as `npm`, `git`, `github`, or `workspace`. Rename a registry that breaks these rules before upgrading, and update the `/~<name>/` URLs that clients use. pnpr refuses to start on such a name in `pnpr.yaml`. With `registriesManagedBy: api`, pnpr starts but answers registry requests with `503` until an admin replaces the stored registries through the admin API.

- The pnpr resolver refuses a request with a field it does not know, with the error code `ERR_PNPR_UNSUPPORTED_REQUEST_FIELD`. A request's `acceptedLockfileFormats` now names formats such as `pnpm-lock/9.0`. Every resolve `done` frame names the format of its lockfile in `lockfileFormat`. A streamed resolve sends a `progress` frame every 15 seconds.

- SCIM-provisioned users can sign in through OIDC with `login.scim: true` when their `externalId` matches their OIDC subject. Artifact access rules now accept `team:<registry>:<teamId>` references to hosted teams.

- Moved two pnpr configuration keys under `storage`:

  - `s3` is now `storage.s3`.
  - `database` is now `storage.database`.

  `storage` takes either the storage directory or a table. In the table, `storage.path` sets the directory. `--storage` overrides `storage.path`.

### Patch Changes

- Fixed `pnpm` and `pnpr` occasionally crashing on the first HTTPS request, mostly on Windows virtual machines under load. They now seed TLS randomness from the operating system's random number generator [pnpm/tasks#239](https://github.com/pnpm/tasks/issues/239).

- `POST /-/pnpr/v0/resolve` and `POST /-/pnpr/v0/verify-lockfile` now limit request bodies to 32 MiB. Concurrent and queued requests are capped per server instance and per caller identity. Streaming resolves abort when the client disconnects. Resolves enforce `requestTimeout` or a 10-minute ceiling through stream completion. Oversized dependency graphs are rejected [zkochan/pnpm-tasks#76](https://github.com/zkochan/pnpm-tasks/issues/76).

- `cargo owner` now reports that owner operations are not supported by the registry.

- The pnpr server now includes a complete `appliedPolicies` echo in successful npm resolution and lockfile verification responses.

- pnpr now advertises capabilities through `pnpr.protocols` at the server root and npm registry bases. pnpm reads this capability map for remote resolution and shared build caches. The HTTP protocol path remains `/v0/`.

- Hosted Python wheel uploads now advertise core metadata with its SHA-256 and serve it at `<filename>.metadata`. Installers can resolve dependencies without downloading the whole wheel.

- pnpr now mints personal bearer tokens with the `pnpr_` prefix, a base62 payload, and an IEEE CRC-32 checksum. `POST /-/npm/v1/tokens` accepts only `"classic"` or `"granular"` for `kind`. Token responses omit `scopes`. pnpr deletes expired bearer tokens from storage at startup and every hour.

  Account records in SQL databases and `auth.users.file` now store `status`, `credential_kind`, nullable `password_hash`, and `created_at` fields. The `tokens` table no longer stores `scopes`. Start upgraded alpha servers with fresh auth state [zkochan/pnpm-tasks#81](https://github.com/zkochan/pnpm-tasks/issues/81).

- Password changes now revoke existing bearer tokens. Users can change their password with `npm profile set password`.

  Failed login attempts now share a per-account limit across client IPs. A full registration limit returns the same authentication error for unknown accounts and incorrect passwords.

  Authentication errors for team administration, password verification, and logout now identify the requested resource or username correctly.

- Accounts and tokens now live in the metadata database, next to package metadata. By default that is `<storage>/.metadata/metadata.sqlite`. The `auth.users.file` and `auth.tokens.file` settings are removed. `pnpr import-htpasswd` writes into the metadata database and no longer takes `--target`.

  With the default SQLite database, a password reset made with `pnpr add-user` next to a running server now revokes the account's tokens on the server's next request [zkochan/pnpm-tasks#85](https://github.com/zkochan/pnpm-tasks/issues/85).

- pnpr database connection diagnostics now include the target and underlying cause, with credentials redacted. `pnpr config check` rejects insecure database token transport. It also rejects database drivers missing from the build. Errors for a newer database schema now advise upgrading pnpr.

- pnpr now rejects corrupted hosted archives before sending download responses, including partial downloads.

- pnpr now rechecks package ownership when another registry replica changes a package or its removal record during a publish, dist-tag change, maintainer update, yank, or unpublish [zkochan/pnpm-tasks#96](https://github.com/zkochan/pnpm-tasks/issues/96).

- `pnpr` now writes a random `store_id` into `layout.json` when initializing a hosted store. The `store_id` is also recorded in the metadata database. `pnpr` refuses to start, collect (`pnpr gc`), verify (`pnpr verify`), or migrate (`pnpr migrate-storage`) when the store IDs in `layout.json` and the metadata database differ or when only one side holds one [zkochan/pnpm-tasks#62](https://github.com/zkochan/pnpm-tasks/issues/62).

- `cache.maxSize`, `cache.maxAge`, and `pnpr cache-gc` now account for and evict across the whole cache directory, including `resolver-store`, `resolver-cache`, and `lockfile-verdicts.sqlite`. pnpr refuses to start without `secret` or `secrets.cache` when the cache persists across restarts, is shared with another process, or uses shared storage or database backends. The default OSV database path moved to `<storage>/.osv/npm/all.zip`.

- `pnpr config check` now validates `database.url` syntax, schemes, connection options, and pool limits for both metadata and identity databases.

- `pnpm login` and `npm login` against pnpr now print a code that the user types in the browser before signing in. A login link alone can no longer sign anyone in.

  A token created without a password, by an account that has none, now expires no later than the session or token that created it. It also stays within that credential's CIDR whitelist.

- Removing the last version of a hosted npm package now removes the package so packument reads return `404` and search omits it. Hosted npm search results now always include a `maintainers` array. Hosted npm search results now project `publisher` to `{"username": ...}` without exposing internal account IDs [zkochan/pnpm-tasks#47](https://github.com/zkochan/pnpm-tasks/issues/47).

- pnpr now rejects nested ecosystem groups in `registries`. Declare registries directly under `registries` with unique names and select their protocol with `ecosystem`. Hosted registries use the default storage namespace when `org` is omitted.

- `pnpr gc --dry-run` no longer lists OCI image blobs as removable while another repository in the same organization still keeps them [zkochan/pnpm-tasks#50](https://github.com/zkochan/pnpm-tasks/issues/50).

- pnpr refuses hosted stores and metadata databases created by earlier alpha releases. Replace `registries.<name>.org` with `registries.<name>.namespace` before starting the upgraded server. Preserve the old hosted store and metadata database, including npm package maintainer lists. Point pnpr at an empty hosted store and metadata database before upgrading. The new store starts a fresh transparency log that does not extend earlier checkpoints. Republish hosted packages after upgrading. Republishing an npm package makes the publisher its sole maintainer. Restore any additional npm package maintainers after republishing.

  Hosted registries with a `namespace` now store archives under `namespaces/<namespace>/<ecosystem>/`. Hosted registries without a `namespace` keep using `default/<ecosystem>/`. SQL metadata now uses the `namespaces` table and `namespace` column.

  `artifacts.owners` replaces `artifacts.orgs` for shared-artifact, compiler-cache, and pipeline-run access rules.

  Publication responses, package release-note listings, and `pnpr gc` audit spans now report `namespace` [zkochan/pnpm-tasks#66](https://github.com/zkochan/pnpm-tasks/issues/66).

- pnpr metadata reads and writes now retry on a fresh connection when `sqld` or Turso expires or drops a libSQL stream mid-transaction [zkochan/pnpm-tasks#44](https://github.com/zkochan/pnpm-tasks/issues/44).

- pnpr now answers the first request after its remote libSQL database sat idle for a few seconds or restarted. Such a request used to fail with HTTP 500, and `/-/ping` and `/-/ready` with 503, on a `STREAM_EXPIRED` or "invalid baton" error from `sqld` or Turso.

- Fixed resolution of private npm packages through the server's own registry URLs. Upstream credentials are no longer sent to the server's public URL.

- Malformed JSON request bodies sent to login, token creation, package publish, packument update, staged publish, and security audit endpoints now return `400 Bad Request`.

  [Related task](https://github.com/zkochan/pnpm-tasks/issues/49).

- Added the `maxTarballSize` setting, the largest tarball pnpr fetches from an upstream registry. It takes a byte size such as `256MiB`. The default is `100MiB`, the limit pnpr applied before. A tarball that declares a size over the limit is refused with a 502 status.

- MySQL support for `database.url` is now marked experimental, and pnpr logs a warning at startup when it is configured. SQLite, libSQL, and PostgreSQL stay stable.

  pnpr now refuses text that holds a NUL character on every database, so a request that SQLite accepts cannot fail on PostgreSQL.

  pnpr instances on different schemas of one PostgreSQL database no longer wait on each other's write and migration locks.

- `POST /-/npm/v1/tokens` now honors `packages_and_scopes_permission` and interprets integer `expires` values in days, matching `npm token create`. Unsupported package, scope, organization, and stage-only token restrictions now return `400 Bad Request`.

- The prebuilt pnpr binaries and the Docker image now support PostgreSQL and MySQL in `database.url`. They refused both at startup.

- Cargo publishes to an unclaimed package now return 403 and name the `packages:` setting in the error. Unsupported npm visibility and collaborator endpoints return an explanatory error.

- Oversized npm publish requests now identify the 100 MiB JSON limit.

- Account creation and renaming now refuse usernames that differ from an existing account only by letter case or accents. pnpr also refuses usernames that mix Unicode scripts or contain restricted Unicode characters. pnpr no longer starts if its accounts file holds two active usernames that look alike or a username that breaks these rules.

- pnpr instances with an embedded libSQL `replicaPath` now read the primary database when a tarball, archive, or exact version lookup misses the local replica. Fetching a newly published tarball from another instance within `syncInterval` used to fail with HTTP 404.

- Resetting admin package rules now reports the version as `config`.

- Running pnpr servers now refuse requests after another server upgrades their database beyond the version they support. Schema migration records now include their application time.

  Database operation timeouts now return `503 Service Unavailable`.

- Added `shutdownTimeout` (`25s` by default) to configure how long pnpr waits for in-flight requests to drain on `SIGINT` or `SIGTERM`. `GET /-/ready` now returns `503 Service Unavailable` as soon as shutdown starts. `GET /-/ping` now returns `{}` without probing storage or auth backends.

- pnpr now limits graceful shutdown to 30 seconds after receiving SIGINT or SIGTERM. Slow clients can no longer delay process termination indefinitely.

- Completed OCI uploads now remove their session records. Garbage collection now reports the reclaimed bytes when it finishes an interrupted reclaim.

- `pnpr` now reclaims abandoned publish staging files once they are older than `requestTimeout` at startup, periodically while serving, and during `pnpr cache-gc`. Cancelled or timed-out publishes also remove their staging files immediately.

- pnpr now re-imports transparency log entries from stored entry bundles when the metadata database is restored from an earlier snapshot than hosted storage. Rolled-back package versions from those entries cannot be republished, and pnpr refuses to start or sign checkpoints when stored log state diverges from the database. `pnpr migrate-storage` also copies transparency log tiles, entry bundles, and checkpoints to S3 [zkochan/pnpm-tasks#65](https://github.com/zkochan/pnpm-tasks/issues/65).

- twine now displays Python upload error reasons without verbose logging over HTTP/1.

- Removing an npm version now drops dist-tags pointing to it. Only `latest` moves to the highest remaining version.

- pnpr limits upstream package metadata responses with `maxPackumentSize`, which defaults to 64 MiB. Cached tarballs stop being served when refreshed metadata removes their versions. Missing tarball integrities are computed in the background and retained across matching refreshes.

- A resolve request no longer sends a private upstream's credential when it fetches a name outside that upstream's `packages:` map. This applies even when the request names the upstream's URL as its registry.

- pnpr refuses SQL metadata and auth databases created by earlier alpha releases. Delete pnpr's SQL tables or point pnpr at empty databases before upgrading, including any local `tokens.db`. SQL token and migration timestamps now use milliseconds since the Unix epoch.

- pnpr continues resolving requests after serving more than 1,024 distinct client configurations. Inactive configurations are reclaimed when the cache is full.

  Excess concurrent npm resolution and verification requests return `503 Service Unavailable` until active work finishes.

- `DELETE /v2/<name>/manifests/<digest>` now answers `403 DENIED` when a retained image index still references the manifest [zkochan/pnpm-tasks#48](https://github.com/zkochan/pnpm-tasks/issues/48).

- With PostgreSQL or MySQL metadata, concurrent publishes and OCI blob uploads no longer fail with HTTP 500. Metadata writes now wait for each other. Concurrent writes used to fail with a serialization error or a deadlock. pnpr now logs PostgreSQL and MySQL errors as `SQL database error`.

- Removed `pnpr export`. Back up the database and hosted storage together to restore a registry.

- Registry-owned endpoints now return JSON errors with `error`, `reason`, and stable `ERR_PNPR_` codes. npm error responses and resolution stream errors include these codes too.
