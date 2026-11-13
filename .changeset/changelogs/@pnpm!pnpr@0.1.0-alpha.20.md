## 0.1.0-alpha.20

pnpr 0.1.0-alpha.20 keeps its registries, accounts, and tokens in the database, so every replica serves the same registries and admins change them without a config deploy. Publishing a new version of a package is now limited to the package's owners by default, and tokens expire after 90 days. It does not migrate alpha.19 data and it renames several config keys, so read the upgrade steps below before you upgrade. It also includes security fixes. A web login link alone can no longer sign anyone in. A private upstream's credential is no longer sent for names outside its `packages:` map.

### Minor Changes

#### Upgrading from alpha.19

- pnpr does not migrate alpha.19 data. At startup it refuses a hosted store, a metadata database, or a SQL auth database that alpha.19 created. To upgrade:

  - Back up the hosted store and the databases, including a local `tokens.db` and the npm package maintainer lists.
  - Point pnpr at an empty hosted store and an empty database.
  - Recreate accounts and tokens. Recreated accounts get new IDs, so update `auth.admins`, access lists, and team rosters.
  - Republish hosted packages. Republishing an npm package makes the publisher its sole maintainer, so restore the other maintainers afterwards.

  The new store starts a fresh transparency log that does not extend earlier checkpoints.

- A config file that declares `registries`, `defaultRegistry`, or `routes.public` is refused at startup. Move those blocks into a file of their own and store it with `pnpr registries import`. In that file:

  - Rename `registries.<name>.org` to `registries.<name>.namespace`.
  - Rename an upstream registry's `maxAge` to `packumentTtl`, the name of the global setting it overrides.
  - Declare every registry directly under `registries` with a unique name, and select its protocol with `ecosystem`. Nested ecosystem groups are refused.
  - Give every registry a name that starts with a letter and contains only letters, digits, `.`, `_`, and `-`. A name cannot be a reserved pnpm prefix name such as `npm`, `git`, `github`, or `workspace`. Update the `/~<name>/` URLs that clients use when you rename a registry.
  - Write an upstream credential as a `${NAME}` reference to an environment variable that the config lists in `registryEnv`. An upstream URL with a user name, password, or query string is refused.
  - Remove `teamsManagedBy` and `rulesManagedBy`. Teams and rule changes stored under those settings are not carried over.
  - Add `publish: $authenticated` to a `packages:` entry that any signed-in account should keep publishing to.

- Renamed and regrouped pnpr configuration keys:

  - `s3` is now `storage.s3`, and `database` is now `storage.database`. `storage` takes either the storage directory or a table. In the table, `storage.path` sets the directory, and `--storage` overrides it.
  - `secrets.cache` and `secrets.oci` are now `secret.cache` and `secret.oci`. `secret` takes either one string or that table. A `secret` that YAML reads as a number must be quoted.
  - `trustedProxies` is now a table. The proxy list moves to `trustedProxies.networks`, and `trustedProxyHeader` is now `trustedProxies.header`.
  - `oci.maxBlobBytes`, `oci.maxManifestBytes`, and `oci.chunkMaxBytes` are now `oci.maxBlobSize`, `oci.maxManifestSize`, and `oci.maxChunkSize`. Byte limits take size strings such as `4MiB`.
  - `artifacts.orgs` is now `artifacts.owners`. `artifacts.quota.ownerGiB` and `artifacts.quota.totalGiB` are now `artifacts.quota.perOwner` and `artifacts.quota.total`.
  - `auth.users.file` and `auth.tokens.file` are removed. Accounts and tokens live in the database, next to package metadata. By default that is `<storage>/.metadata/metadata.sqlite`.
  - `log.type` is removed. pnpr always logs to stdout.

- pnpr now derives the private cache key from `secret`. Private cache entries written by an earlier version are no longer used, and `pnpr cache-gc` removes them. pnpr refuses to start without `secret` or `secret.cache` when the cache directory holds data from a previous run, is shared with another process, or the server uses S3, libSQL, PostgreSQL, or MySQL.

- Removed `pnpr export`. Back up the database and hosted storage together to restore a registry.

- `pnpr import-htpasswd` writes into the database and no longer takes `--target`.

- The default OSV database path moved to `<storage>/.osv/npm/all.zip`. `--storage` no longer replaces a `cache` path set in the config.

#### Registries and package owners

- pnpr now keeps its registries in the database. Every replica serves the same registries. Admins listed in the new `auth.registryAdmins` setting change them through `/-/pnpr/v0/admin/registries` without a config deploy. Each change is a revision that can be restored.

  `pnpr registries export` prints the stored registries as a file that `pnpr registries import` accepts. `pnpr registries import --check` fails when the stored registries differ from a file.

  On its first start against an empty database, pnpr stores a default set of registries: a proxy of the public npm registry and a hosted registry for private packages.

  The team endpoints, `npm access grant`, and the rules endpoints now edit the stored registries.

- A `packages:` entry without a `publish` list now admits only the package's owners and `auth.admins`. The account that first publishes a package, crate, Python project, or image repository becomes its owner. An npm package's owners are its maintainers. Before, any signed-in account could publish a new version of another account's package and move its dist-tags.

- `cargo owner` now lists, adds, and removes the owners of a hosted crate. Python projects and image repositories take the same requests at `/pypi/owners/<project>` and `/oci/owners/<repository>`. A package's owners and `auth.admins` can change its owners.

#### Accounts and audit logs

- Tokens from a password login, and tokens from `npm token create` without an expiry, now expire after 90 days. The `auth.tokens.defaultLifetime` setting changes this lifetime.

  Added the `auth.tokens.maxLifetime` setting. It limits how long any token from a password login or `npm token create` stays valid, for example `90d`.

- Added `POST /-/pnpr/v0/admin/users` to create accounts through the admin API. `PUT /-/pnpr/v0/admin/users/{id}` no longer creates accounts. It replaces an existing account's password by account ID and returns `404` for unknown IDs.

- SCIM-provisioned users can sign in through OIDC with `login.scim: true` when their `externalId` matches their OIDC subject.

- pnpr audit records now carry `actor_id`, the account ID of whoever performed the action, which stays the same when the account is renamed. Sign-ins, failed logins, workload credential use, token creation and revocation, and publishes are now audited as `auth.sign_in`, `auth.login_failed`, `auth.workload_use`, `token.create`, `token.revoke`, and `package.publish`. Version and publication note events now name the actor by account name, and every event about another account names it with both `user` and `user_id`.

#### Settings and clients

- New `listen` and `publicUrl` keys set the bind address and the public URL. `--listen` and `--public-url` override them. pnpr logs a warning when it listens on a non-loopback address without `publicUrl`.

- Added `cloudflareCdn` to purge cached public packages after publishes, unpublishes, deprecations, and access-rule changes. The Cloudflare cache example serves anonymous cache hits without contacting the registry and limits cached responses to five minutes.

- An unset environment variable in `storage.database.url` now fails startup, as it already did in the other credential fields. Config errors now give the line and column without quoting the surrounding lines of the file. An unparsable `routes.public` registry URL or package glob is now refused. Before, pnpr logged a warning and ignored the rule.

- The pnpr resolver refuses a request with a field it does not know, with the error code `ERR_PNPR_UNSUPPORTED_REQUEST_FIELD`. A request's `acceptedLockfileFormats` now names formats such as `pnpm-lock/9.0`. Every resolve `done` frame names the format of its lockfile in `lockfileFormat`. A streamed resolve sends a `progress` frame every 15 seconds.

### Patch Changes

#### Security and access

- `pnpm login` and `npm login` against pnpr now print a code that the user types in the browser before signing in. A login link alone can no longer sign anyone in.

  A token created without a password, by an account that has none, now expires no later than the session or token that created it. It also stays within that credential's CIDR whitelist.

- A resolve request no longer sends a private upstream's credential when it fetches a name outside that upstream's `packages:` map. This applies even when the request names the upstream's URL as its registry. Upstream credentials are also no longer sent to the server's public URL.

- pnpr now rechecks package ownership when another registry replica changes a package or its removal record during a publish, dist-tag change, maintainer update, yank, or unpublish.

- Password changes now revoke existing bearer tokens. Users can change their password with `npm profile set password`. With the default SQLite database, a password reset made with `pnpr add-user` next to a running server now revokes the account's tokens on the server's next request.

- Failed login attempts now share a per-account limit across client IPs. A full registration limit returns the same authentication error for unknown accounts and incorrect passwords.

- Account creation and renaming now refuse usernames that differ from an existing account only by letter case or accents. pnpr also refuses usernames that mix Unicode scripts or contain restricted Unicode characters.

- pnpr now mints personal bearer tokens with the `pnpr_` prefix, a base62 payload, and an IEEE CRC-32 checksum. Token responses omit `scopes`. pnpr deletes expired bearer tokens from storage at startup and every hour.

- `POST /-/npm/v1/tokens` now honors `packages_and_scopes_permission` and interprets integer `expires` values in days, matching `npm token create`. It accepts only `"classic"` or `"granular"` for `kind`. Unsupported package, scope, organization, and stage-only token restrictions now return `400 Bad Request`.

#### Publishing npm packages

- Removing the last version of a hosted npm package now removes the package, so packument reads return `404` and search omits it. Removing an npm version now drops the dist-tags pointing to it. Only `latest` moves to the highest remaining version.

- Hosted npm search results now always include a `maintainers` array. They project `publisher` to `{"username": ...}` without exposing internal account IDs.

- Malformed JSON request bodies sent to login, token creation, package publish, packument update, staged publish, and security audit endpoints now return `400 Bad Request`.

- Oversized npm publish requests now identify the 100 MiB JSON limit.

- pnpr now reclaims abandoned publish staging files once they are older than `requestTimeout`. It does so at startup, periodically while serving, and during `pnpr cache-gc`. Cancelled or timed-out publishes also remove their staging files immediately.

- Unsupported npm visibility and collaborator endpoints return an explanatory error.

- Authentication errors for team administration, password verification, and logout now identify the requested resource or username correctly.

#### Other ecosystems

- `pnpr gc --dry-run` no longer lists OCI image blobs as removable while another repository in the same namespace still keeps them.

- `DELETE /v2/<name>/manifests/<digest>` now answers `403 DENIED` when a retained image index still references the manifest.

- Completed OCI uploads now remove their session records. Garbage collection now reports the reclaimed bytes when it finishes an interrupted reclaim.

- Hosted Python wheel uploads now advertise core metadata with its SHA-256 and serve it at `<filename>.metadata`. Installers can resolve dependencies without downloading the whole wheel.

- twine now displays Python upload error reasons without verbose logging over HTTP/1.

- Cargo publishes to an unclaimed package now return 403 and name the `packages:` setting in the error.

#### Databases and storage

- The prebuilt pnpr binaries and the Docker image now support PostgreSQL and MySQL in `storage.database.url`. Before, they refused both at startup.

- With PostgreSQL or MySQL metadata, concurrent publishes and OCI blob uploads no longer fail with HTTP 500. Metadata writes now wait for each other. Concurrent writes used to fail with a serialization error or a deadlock.

- pnpr now answers the first request after its remote libSQL database sat idle for a few seconds or restarted. Such a request used to fail with HTTP 500, and `/-/ping` and `/-/ready` with 503, on a `STREAM_EXPIRED` or "invalid baton" error from `sqld` or Turso. Metadata reads and writes also retry on a fresh connection when the stream expires or drops mid-transaction.

- pnpr instances with an embedded libSQL `replicaPath` now read the primary database when a tarball, archive, or exact version lookup misses the local replica. Fetching a newly published tarball from another instance within `syncInterval` used to fail with HTTP 404.

- MySQL support for `storage.database.url` is now marked experimental, and pnpr logs a warning at startup when it is configured. SQLite, libSQL, and PostgreSQL stay stable.

  pnpr now refuses text that holds a NUL character on every database, so a request that SQLite accepts cannot fail on PostgreSQL.

  pnpr instances on different schemas of one PostgreSQL database no longer wait on each other's write and migration locks.

- Running pnpr servers now refuse requests after another server upgrades their database beyond the version they support. Errors for a newer database schema now advise upgrading pnpr. Database operation timeouts now return `503 Service Unavailable`.

- `pnpr config check` now validates `storage.database.url` syntax, schemes, connection options, and pool limits. It rejects insecure database token transport and database drivers missing from the build. Database connection diagnostics now include the target and underlying cause, with credentials redacted.

- pnpr now writes a random `store_id` into `layout.json` when it initializes a hosted store, and records it in the database. `pnpr`, `pnpr gc`, `pnpr verify`, and `pnpr migrate-storage` refuse to run when the two store IDs differ or when only one side holds one.

- pnpr now rejects corrupted hosted archives before sending download responses, including partial downloads.

- pnpr now re-imports transparency log entries from stored entry bundles when the database is restored from an earlier snapshot than hosted storage. Rolled-back package versions from those entries cannot be republished, and pnpr refuses to start or sign checkpoints when stored log state diverges from the database. `pnpr migrate-storage` also copies transparency log tiles, entry bundles, and checkpoints to S3.

#### Caching and upstream registries

- `cache.maxSize`, `cache.maxAge`, and `pnpr cache-gc` now account for and evict across the whole cache directory, including `resolver-store`, `resolver-cache`, and `lockfile-verdicts.sqlite`.

- Added the `maxTarballSize` setting, the largest tarball pnpr fetches from an upstream registry. It takes a byte size such as `256MiB`. The default is `100MiB`, the limit pnpr applied before. A tarball that declares a size over the limit is refused with a 502 status.

- pnpr limits upstream package metadata responses with `maxPackumentSize`, which defaults to 64 MiB. Cached tarballs stop being served when refreshed metadata removes their versions. Missing tarball integrities are computed in the background and retained across matching refreshes.

#### Resolving for pnpm clients

- pnpr now advertises capabilities through `pnpr.protocols` at the server root and npm registry bases. pnpm reads this capability map for remote resolution and shared build caches. The HTTP protocol path remains `/v0/`.

- The pnpr server now includes a complete `appliedPolicies` echo in successful npm resolution and lockfile verification responses.

- Fixed resolution of private npm packages through the server's own registry URLs.

- `POST /-/pnpr/v0/resolve` and `POST /-/pnpr/v0/verify-lockfile` now limit request bodies to 32 MiB. Concurrent and queued requests are capped per server instance and per caller identity, and excess requests return `503 Service Unavailable` until active work finishes. Streaming resolves abort when the client disconnects. Resolves enforce `requestTimeout` or a 10-minute ceiling through stream completion. Oversized dependency graphs are rejected.

- pnpr continues resolving requests after serving more than 1,024 distinct client configurations. Inactive configurations are reclaimed when the cache is full.

- Registry-owned endpoints now return JSON errors with `error`, `reason`, and stable `ERR_PNPR_` codes. npm error responses and resolution stream errors include these codes too.

#### Running the server

- Fixed `pnpr` occasionally crashing on the first HTTPS request, mostly on Windows virtual machines under load. It now seeds TLS randomness from the operating system's random number generator [pnpm/tasks#239](https://github.com/pnpm/tasks/issues/239).

- Added `shutdownTimeout` (`25s` by default) to configure how long pnpr waits for in-flight requests to drain on `SIGINT` or `SIGTERM`. Slow clients can no longer delay process termination indefinitely. `GET /-/ready` now returns `503 Service Unavailable` as soon as shutdown starts. `GET /-/ping` now returns `{}` without probing storage or auth backends.
