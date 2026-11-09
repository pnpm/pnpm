## 0.1.0-alpha.19

pnpr 0.1.0-alpha.19 stores package metadata in PostgreSQL or MySQL as well as SQLite, stores identical archives once, and adds `pnpr gc` to reclaim unreferenced archives while serving. It does not migrate alpha.18 data, so read the upgrade steps below before you upgrade. It also includes a security fix: pnpr no longer logs the credentials of some malformed URLs. The release also tightens account, team, and publish validation.

### Minor Changes

#### Upgrading from alpha.18

- pnpr does not migrate alpha.18 data. At startup it refuses an alpha.18 metadata database or SQL auth database, including a local `tokens.db`, and leaves it unchanged. To upgrade:

  - Back up these databases, then delete pnpr's tables or point pnpr at new, empty databases.
  - Point `storage` or the `s3:` prefix at a fresh location. Hosted archives use a new layout. A fresh local `storage` directory also gives you a fresh SQLite metadata file.
  - Replace the `backend` and `metadata` blocks with `database.url`. pnpr refuses to start while either block is present.
  - Rename `auth.htpasswd` to `auth.users`. If accounts stay in the local account file, which is the case without `database.url` or with a `sqlite:` one, run `pnpr import-htpasswd --source <old htpasswd>` to bring over existing passwords. With any other `database.url`, recreate the accounts.
  - Passwords now need at least 8 characters, and pnpr checks this at login too. Run `pnpr add-user <username>` to set a new password for an account with a shorter one.
  - List account IDs, not usernames, in `auth.admins`, access lists, and OIDC bindings. Give each entry under `teams` a `name` and `members`.
  - Replace `pnpr oci-gc` and `pnpr verify --prune-orphans` with `pnpr gc`.
  - Recreate accounts and tokens, republish packages, and reconcile the SCIM directory with the identity provider. Recreated accounts get new IDs.

- pnpr now runs numbered schema migrations for its metadata and auth tables. Each configured SQL database records its applied steps in its own `schema_migrations` table, starting at version `1`.

#### Storage

- pnpr can store package metadata in PostgreSQL and MySQL. Set `database.url` to share accounts, tokens, and package metadata in one database.

- A hosted registry stores identical bytes once per org and ecosystem, whichever packages, versions, or image repositories publish them. A publish whose bytes are stored already stores nothing [pnpm/tasks#269](https://github.com/pnpm/tasks/issues/269).

- `pnpr gc` removes the archives that no hosted package maps, in every hosted npm, PyPI, Cargo, and OCI registry. It runs while the registry keeps serving and other replicas keep publishing. It removes the bytes of unpublished versions and packages, image blobs that no retained manifest reaches, and the archives of publishes that stopped or were refused at their commit. It also finishes interrupted reclaims. The new `gc.lease` setting sets how long an interrupted reclaim is left before another run finishes it. It defaults to `1h`.

  `pnpr verify` no longer removes orphaned blobs. It still fails on missing or corrupted npm package blobs, and reports orphaned blobs in every hosted registry. Its `--prune-orphans` and `--min-age` options are removed.

#### Clients

- The server applies the `projectScopedOptionalPeers` setting that a client sends, and records it in the lockfile it returns.

- Added `oci.chunkMaxBytes` to advertise an upload chunk size to OCI clients that support `OCI-Chunk-Max-Length`.

### Patch Changes

#### Security and access

- pnpr no longer logs the credentials of a URL whose `?` or `#` appears before `@`. It now redacts such a URL as `<scheme>://redacted` [pnpm/tasks#301](https://github.com/pnpm/tasks/issues/301).

- Renaming an account preserves its permissions and package ownership. Reusing a deleted username or team name starts with fresh permissions.

- User lookups now require authentication. Team names and rosters are visible only to registry team members and administrators. New accounts cannot use usernames that look like an existing username.

- pnpr now refuses stale authorization when a team roster, managed access rule, or SCIM directory read exceeds five seconds. A stalled roster or rule read no longer delays SCIM refreshes [#17836](https://github.com/pnpm/pnpm/issues/17836).

- Fixed MySQL-backed authentication failing to read case-sensitive usernames.

- pnpr now keeps package ownership after unpublish, team rosters, rule overrides, SCIM users, and staged-publish metadata in its metadata database. An unpublish records ownership and removed versions atomically.

#### Publishing npm packages

- npm publishes now reject package names that npm disallows for new packages, including uppercase letters and an `@` in the package part. Existing upstream packages remain readable.

- pnpr rejects publishes when a version key or manifest version is not valid semver [pnpm/tasks#267](https://github.com/pnpm/tasks/issues/267). Publishing a version key with build metadata returns an explicit error. `DELETE` requests for invalid versions return `400`.

- A tarball `DELETE` of a version that the package still lists unpublishes that version.

- Retrying an identical publish of an unpublished version now returns `403`. Batch publish retries also fail if any published member has been unpublished.

- Stale partial unpublish and deprecation requests now fail with `409`. Deprecation requests preserve dist-tags and other package metadata.

- Dist-tag writes now reject names that parse as semver ranges and targets that are not published versions. Removing `latest` is rejected while published versions exist.

- Release note edits and deletions no longer retain earlier text for restoration.

#### Other ecosystems

- Deleting the last manifest of an OCI repository now removes the repository from `/v2/_catalog`, and `GET /v2/<name>/tags/list` answers `404 NAME_UNKNOWN` until another manifest is pushed. `DELETE /v2/<name>/blobs/<digest>` now answers `403 DENIED` when a retained manifest still references the blob.

- Deleting an OCI manifest by digest now invalidates the saved batch publish outcomes that include it. Deleting an OCI tag leaves batch replay intact. A deleted OCI manifest can be published again.

- The Python registry proxy now supports core-metadata declarations and caches `.metadata` downloads. pip and uv can read dependency metadata without downloading whole wheels.

- Python uploads now reject unreadable archives and metadata that names a different project or version. Publishers can change the yank status of individual files. Administrators can delete files from the index and disable their downloads. Deletion retains stored archive bytes.

- Cargo package claims now cover both `-` and `_` spellings. Requests for a hosted crate cannot fall through to an upstream registry under the alternate spelling. New versions must use the spelling of the crate's first published version.

- Cargo sparse index files now send ETags and return `304 Not Modified` when the client already has the current content.

#### Speed and caching

- Large package metadata requests no longer block unrelated registry requests. Repeated requests reuse rendered metadata while the package content is unchanged.

- Sped up publishing with an `Idempotency-Key` header.

- Responses for public packages read through a named registry, such as `/npm/~npmjs/<package>`, are now cacheable by shared HTTP caches. Packages that need authentication still get `Cache-Control: private, no-store`. PyPI project pages now send `Vary: Accept`.

- A tarball that pnpr downloads from an upstream registry is no longer cut off after the upstream `timeout` while it is still arriving. The `timeout` now limits how long pnpr waits for the upstream to answer and for each further part of the download. Before, a tarball that took longer than 30 seconds to download from a slow upstream failed every time.

- Response body timeouts no longer open an upstream's circuit breaker. Circuit-open responses name the upstream and include `Retry-After`.

#### Responses and logs

- npm clients now display the server's explanation when a registry request fails. Authentication errors distinguish invalid tokens from missing credentials. A `401` response includes a `WWW-Authenticate` challenge.

- Package metadata responses no longer include the internal `formatVersion` field or `access: null`.

- Fixed `npm search` failing on hosted packages with maintainer names. Search results for hosted packages no longer link to an external package website.

- pnpr logs scoped package URLs such as `https://registry.npmjs.org/@babel/cli` with their registry host intact. It previously logged them as `https://redacted@babel/cli`.
