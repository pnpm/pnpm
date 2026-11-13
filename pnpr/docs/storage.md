---
id: storage
title: Storage backends (S3 / R2)
---

pnpr keeps two kinds of data:

- **Hosted**: the source of truth. It holds packages published to this server plus
  anything served in static mode. Lives under `storage`. The bodies of packages
  awaiting approval through [staged publishing](endpoints.md#staged-publishing-endpoints)
  are held under `.staged/`. Their approval records live in the database.
- **Cache**: the disposable mirror of upstream registries plus the resolver
  cache, lockfile-verdict cache, local shared-artifact store, and S3 upload
  staging scratch. Lives under `cache` (defaults to
  `<storage>/.pnpr-cache`).

Package metadata responses also use a bounded memory cache. Repeated requests
reuse the response while the package content, tarball metadata, and registry
URLs are unchanged. Large metadata requests leave the server available to
handle unrelated requests.

## Layout

The hosted store has the same layout on disk and in a bucket:

```text
<storage>/
  layout.json                         marks the layout version and store id
  default/<ecosystem>/                registries in the default namespace
  namespaces/<namespace>/<ecosystem>/ one partition per namespace and ecosystem
<partition>/archives/<shard>/<identity>.<incarnation>
```

`<ecosystem>` is `npm`, `cargo`, `pypi`, or `oci`. A hosted registry without
a `namespace` uses `default/<ecosystem>/`. Set `namespace` explicitly to store
its packages under `namespaces/<namespace>/<ecosystem>/`. In a namespace name,
lowercase letters, digits, `.`, `_` and `-` are kept. Every other byte is
written as `!` and two hex digits.

A namespace stores the bytes of an archive once, by their identity: the digest
its ecosystem checks them by, as `<algorithm>-<hex>`. That is the `sha512` of
npm's `dist.integrity`, and the `sha256` of an image blob's digest, a crate's
checksum, or a Python distribution's hash. `<shard>` is the first two hex
digits of the digest, so `sha512-ab12…` is under `archives/ab/`. A public file
name, such as `left-pad-1.0.0.tgz`, never reaches the store: the
[database](#database) maps each file of a package to the identity of
its bytes, and a download of the file reads the bytes through that mapping.
Files with the same bytes, of one package or of several, share one stored
archive.

pnpr verifies a hosted archive before starting its first download response.
Unchanged archives reuse that verification when the operating system provides a
reliable file change timestamp. On Windows, pnpr reads and hashes the complete
archive before every download, including partial downloads.
A first download from object storage uses temporary disk space under the cache
while pnpr checks the complete archive, including when the client requested only
a byte range. pnpr verifies at most four object-store downloads in parallel. A
request waiting more than five seconds for verification returns `503 Service
Unavailable`.

Temporary download files share a 1 GiB admission threshold, reserved in 1 MiB
units until the response finishes or disconnects. An archive larger than 1 GiB
reserves the threshold exclusively and can use more than 1 GiB of disk space.
The total size of these temporary files is bounded by the greater of 1 GiB and
the largest active archive download. Requests that cannot reserve space return
`503 Service Unavailable` immediately. Corrupted archives also return
`503 Service Unavailable`.

Provision the cache filesystem with enough free space for the largest published
archive, including blobs allowed by the [OCI size limit](configuration.md#oci).

`<incarnation>` counts the times the bytes were stored, from `1`. Bytes a
[reclaim](cli.md#gc) removed and a later publish stores again take the next
incarnation, so a key is never written again once a reclaim may remove it.

When pnpr initializes an empty hosted store and database, it writes a
random `store_id` into `layout.json` and into the database's
`hosted_store` table. pnpr refuses to start, collect (`pnpr gc`), verify
(`pnpr verify`), or migrate (`pnpr migrate-storage`)
when `layout.json` and `hosted_store` hold different store IDs or when only one
side holds one. It also refuses a store written by a newer pnpr, or one in an
earlier layout, including the one pnpr used before `layout.json` existed: move
to a fresh storage directory or bucket prefix, wipe pnpr's metadata tables, and
republish. A cache in another layout is emptied on startup and refilled on
demand.

## The database {#database}

pnpr keeps package metadata, accounts, registries, and the rest of its state
in a SQL database, by default an embedded SQLite database at
`<storage>/.metadata/metadata.sqlite`. The `storage.database` block names another database by URL:

```yaml title="pnpr.yaml"
storage:
  path: ./storage
  database:
    # Optional. A relative path is resolved against the config file's directory.
    url: sqlite:./metadata.sqlite
```

Archives stay in the hosted store. The database holds every hosted package's
metadata in every ecosystem, former maintainers and unpublished versions, the
[registries kept in the database](configuration.md#registries-in-the-database),
SCIM users, and staged-publish approval records. An
unpublish records former maintainers and removed versions in one transaction.
Back up this database for package ownership and managed access settings,
alongside the hosted store for archives and staged bodies. Transparency log
tiles and checkpoints, and pipeline run streams also stay in the hosted store.

pnpr keeps metadata only in a database: a `storage.s3`
block needs a [shared database](auth-backends.md), and pnpr refuses to start on a
`storage.database` block that still names a `backend`. Earlier releases could keep
each package's metadata as a document beside its archives. pnpr does not read
those documents. A filesystem store from then starts with empty metadata. An
`storage.s3` store from then is refused at startup until `storage.database.url` names a
shared database, and then starts with empty metadata. Either way, republish
its packages into a fresh storage directory or bucket prefix: a store in an
earlier layout is refused.

With SQL metadata:

- A published npm version's manifest cannot change. A write that would change
  it fails with `409`. Deprecating or undeprecating a version, moving a
  dist-tag, and unpublishing still work.
- A version is stored as the manifest
  [accepted from its archive](endpoints.md#publish-time-manifest-verification),
  under the interpretation profile it was accepted under and with the digests
  of the archive's `package.json` and `npm-shrinkwrap.json`.
- The database keeps every unpublished version, including the versions of an
  unpublished package, and refuses to accept one again with `403`. Former
  maintainers retain their claim to the package name in the same database.
- A dist-tag change rewrites only that tag, and pnpr keeps the history of
  every move of a tag: when it moved, from which version to which, and who
  moved it. A publish, a document update, and a dist-tag change each record
  the user who made them.
- Every change to a namespace's packages advances that namespace's generation.
  A hosted npm package response, and the response to a dist-tag change, carry
  it in a `pnpr-generation` header as `<namespace>:<generation>`, with an empty
  namespace for registries in the default namespace. Generations of different
  namespaces are unrelated.
- pnpr renders package documents from the database and caches them by
  version, so a dist-tag change or a new release does not re-render the
  versions that did not change.
- A reader sees all of a publish, including a
  [batch](ecosystems.md#one-publish-for-a-workspace-that-spans-ecosystems), or
  none of it. pnpr uploads the archives first and then commits the metadata in
  one transaction. A retried publish gets its original response instead of a
  `409`. Each publish is recorded as a
  [publication](endpoints.md#publication-endpoints).

SQLite metadata serves one pnpr process. Reads use their own database
connections, so a read never waits for a write. A `sqlite:` URL cannot be
combined with a `storage.s3` block. Back the database file up together with the
hosted store.

The SQLite and libSQL metadata tables use rowid storage for package state,
accepted manifests, READMEs, release records, and log entries. Large payloads
stay out of the interior pages used to look up their keys.

SQL timestamps use milliseconds since the Unix epoch and the server's
application clock. Package publication times must use RFC 3339. Responses
render them in UTC with millisecond precision.

Release notes retain their current text and publish-time digest. Edits and
deletions do not retain earlier text for restoration.

### Schema migrations {#metadata-schema-changes}

pnpr creates its metadata and auth tables in an empty database and records each
applied migration step in `schema_migrations`, starting with step `1` for the
1.0 schema. Its `applied_at` column holds milliseconds since the Unix epoch.
Running servers probe the primary database version at most once per second.
Requests use the last compatible result while a probe runs or the primary is
unreachable. After a probe detects a version newer than the server supports,
that server refuses further requests except liveness probes until it restarts.

When pnpr opens a database at an older numbered version, it
takes the database write lock and runs each pending migration step in order.
Steps run in one transaction on SQLite, libSQL, and PostgreSQL, and record
their version in `schema_migrations` after completing on MySQL.

pnpr refuses a database from a newer release. It refuses a database whose
`schema_migrations` table cannot be read, or that records a version but lacks
some of pnpr's tables. It also refuses a pre-1.0 database without
`schema_migrations` (such as one holding `schema_version`,
`auth_schema_migrations`, or unmarked tables) before changing anything in it.
To replace a pre-1.0 database, drop its pnpr tables (or delete the SQLite file
and its `-wal` and `-shm` files) together with the hosted store, or move to a
fresh hosted store (such as a new storage directory or bucket prefix) and a
fresh database (moving the storage directory replaces the default
SQLite file only when `storage.database.url` is not set), so pnpr initializes both with
a new store ID.

## PostgreSQL and MySQL {#postgresql-and-mysql}

Set `storage.database.url` to a `postgres://` or `mysql://` URL to store package
metadata, accounts, and tokens in the same shared database. Both work with a
filesystem hosted store or S3. MySQL support is experimental. See
[Shared database](auth-backends.md) for connection settings and the MySQL
key-size limit.

pnpr refuses text that holds a NUL character on every database, because
PostgreSQL cannot store it.

Publication writes commit atomically. Metadata writes from every pnpr instance
on the database wait their turn on a database lock, and each metadata read sees
a committed snapshot.

## libSQL {#libsql-database}

Several pnpr instances share one database when its URL names a
libSQL server, such as [Turso](https://turso.tech) or a self-hosted `sqld`.
Every behavior of [the database](#database) applies. A libSQL URL
works with a `storage.s3` block, so archives stay in the bucket:

```yaml title="pnpr.yaml"
storage:
  s3:
    bucket: my-pnpr-packages
  database:
    url: libsql://my-db.turso.io
    authToken: ${TURSO_AUTH_TOKEN}
    # Optional. Reads views from a local copy instead of the primary.
    replicaPath: ./metadata-replica.db
    # Optional. Defaults to 60s.
    syncInterval: 10s
    # Optional. Defaults to 10.
    maxConnections: 10
```

`url` also takes `http://` or `https://` for a self-hosted `sqld`. pnpr
refuses to start when `authToken` names an unset environment variable. It
sends the token only over HTTPS, or over HTTP to a loopback host.

Every write goes to the primary. Without `replicaPath`, every read does too.
With `replicaPath`, pnpr keeps an embedded replica of the database in that
file and pulls the primary's changes every `syncInterval`:

- Package documents and listings are read from the replica. A change made
  through another pnpr instance shows up within `syncInterval`.
- Archives and exact version metadata are read from the replica first, and
  from the primary when the replica does not have them yet.
- An instance always sees its own changes.
- Permission checks, the reads a write depends on, and the response to a
  retried publish read the primary.
- When the replica cannot sync, pnpr reads the primary.

### Metadata concurrency {#metadata-concurrency}

Two instances publishing to one package at the same moment both land. The
publish that commits second finds the package at another generation, rebases
its versions, tags, and other changes onto what the first committed, and
commits again, as if the two had run one after the other. A version, tag, or
time the first added stays; any other field the second changed is taken whole.
When the package's maintainers changed meanwhile, for example because another
user created the package first, the second publish is refused with `409`
instead.

## Backends

By default both are local directories. Adding a `storage.s3` block moves the
**hosted** store and the enabled **shared-artifact** store into an S3-compatible
object store, so they can be shared by several stateless pnpr replicas. Their
metadata then lives in a [shared database](auth-backends.md), using libSQL,
PostgreSQL, or MySQL. The ordinary upstream and resolver cache stays on local disk.

Shared artifacts use a reserved `.artifacts/` namespace under the
configured prefix; hosted package objects keep their normal namespace. Without
S3, artifacts live under `<cache>/shared-artifacts`. They are rebuildable
cache data rather than authoritative packages, but publication slots are
immutable while they exist.

Because any S3-compatible endpoint works, this also covers **Cloudflare R2**,
**MinIO**, **Backblaze B2**, **Wasabi**, and so on. Point `endpoint` at the right
host.

```yaml title="pnpr.yaml"
storage:
  path: ./storage
  s3:
    bucket: my-pnpr-packages
    region: auto
    # Omit `endpoint` for AWS S3. For R2 use the account endpoint:
    endpoint: https://<account-id>.r2.cloudflarestorage.com
    # Optional key prefix, so one bucket can hold more than the hosted store:
    prefix: packages
    # Credentials. Omit these to fall back to the standard
    # AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY environment variables.
    accessKeyId: ${PNPR_S3_ACCESS_KEY_ID}
    secretAccessKey: ${PNPR_S3_SECRET_ACCESS_KEY}
  database:
    # Example shared database for hosted storage on S3.
    url: libsql://<your-db>.turso.io
    authToken: ${PNPR_LIBSQL_TOKEN}
# cache: ./cache     # local proxy cache + resolver cache + S3 upload staging
```

## Options

| Key | Required | Description |
| --- | --- | --- |
| `bucket` | yes | Bucket the hosted packages and enabled shared artifacts are stored in. |
| `region` | no | AWS S3 needs a real region (e.g. `us-east-1`); Cloudflare R2 uses `auto`. |
| `endpoint` | no | Custom endpoint for S3-compatible providers. Omit for AWS S3; for R2 it's `https://<account-id>.r2.cloudflarestorage.com`; for MinIO it's e.g. `http://127.0.0.1:9000`. |
| `prefix` | no | Key prefix every object is stored under. |
| `accessKeyId` | no | Access key. Falls back to `AWS_ACCESS_KEY_ID` when unset. |
| `secretAccessKey` | no | Secret key. Falls back to `AWS_SECRET_ACCESS_KEY` when unset. |
| `forcePathStyle` | no | Boolean. `true` uses path-style addressing (`endpoint/bucket/key`), and `false` uses virtual-hosted addressing (`bucket.endpoint/key`). When omitted, `AWS_VIRTUAL_HOSTED_STYLE_REQUEST=true` selects virtual-hosted addressing, and otherwise pnpr uses path-style addressing, which AWS, R2, and MinIO accept. |
| `allowHttp` | no | Boolean. Allow plain-HTTP endpoints, needed for a local MinIO over `http://`. Defaults to `false` (HTTPS only). |

## Concurrent writers {#concurrent-writers}

Several stateless pnpr replicas can share one bucket and database.

- **Package metadata**: lives only in the database. Publishes, unpublishes,
  and dist-tag changes commit in database transactions. Concurrent changes
  are checked against the package generation to prevent lost updates. See
  [metadata concurrency](#metadata-concurrency).
- **Archives**: an archive is stored under the identity of its bytes and an
  incarnation the database counts (see [Layout](#layout)), so two
  replicas that store the same key store the same bytes, and a store needs no
  compare-and-swap. A publish reads the row of the bytes it is about to store
  with the package's document: when they are stored already it stores nothing,
  and otherwise it stores the next incarnation. Its commit is refused with
  `409` unless that incarnation is still the live one, or is the next one and
  the commit creates it, so a publish never commits a file whose bytes a
  [reclaim](cli.md#gc) is removing. A retry stores the bytes afresh.
- **Shared artifacts**: envelopes and blobs up to 8 MiB use create-only
  writes, and the quota counter uses conditional updates across replicas. A
  larger blob is uploaded in parts and committed only once its bytes match its
  integrity, so a concurrent upload of it can only store the same bytes. A
  publication whose compatibility set overlaps an existing artifact for the
  same input key gets a `409`; a byte-identical retry succeeds. After an
  ambiguous object-store write failure, pnpr reclaims blobs referenced by no
  stored envelope and rebuilds quota once active publications drain. An upload
  that fails and cannot be aborted leaves incomplete parts that pnpr cannot
  list, so give the bucket a lifecycle rule that aborts incomplete multipart
  uploads.

This applies to the S3 backend. The local filesystem backend keeps its
single-process behavior, since the shared-store race is specific to replicas
sharing one object store.

## A complete Cloudflare R2 example

```yaml title="pnpr.yaml"
storage:
  path: ./storage
  s3:
    bucket: my-pnpr-packages
    region: auto
    endpoint: https://abc123def456.r2.cloudflarestorage.com
  database:
    url: libsql://my-pnpr.turso.io
    authToken: ${PNPR_LIBSQL_TOKEN}

artifacts:
  enabled: true
```

The registries live in the shared database with the package metadata. The
first start stores the
[default registries](configuration.md#registries-in-the-database), and
[`pnpr registries import`](cli.md#registries) replaces them with your own.

```sh
export AWS_ACCESS_KEY_ID="<r2-access-key-id>"
export AWS_SECRET_ACCESS_KEY="<r2-secret-access-key>"
export PNPR_LIBSQL_TOKEN="<turso-auth-token>"
pnpr -c ./pnpr.yaml --listen 0.0.0.0:7677 --public-url https://registry.example.com
```

`--public-url` rewrites the `dist.tarball` URLs in served packuments, so clients
fetch tarballs back through this server rather than the upstream.

## A local MinIO over plain HTTP

MinIO over `http://` needs `allowHttp`:

```yaml title="pnpr.yaml"
storage:
  s3:
    bucket: pnpr
    region: us-east-1
    endpoint: http://127.0.0.1:9000
    forcePathStyle: true
    allowHttp: true
    accessKeyId: minioadmin
    secretAccessKey: minioadmin
```

## S3 publishing and lifecycle caveats

When publishing packages to an S3 backend, pnpr:

1. **Stages locally**: Incoming blobs are staged under `<cache>/.staging/`.
2. **Uploads the blobs**: pnpr uploads each blob whose bytes the namespace does not hold live yet under its next incarnation; bytes it holds are not uploaded again.
3. **Commits the metadata**: pnpr commits every package of the publish to the database in one transaction.

Publishes of different packages can upload concurrently. Metadata commits share
the database's write lock. If archive reclamation races an npm
publish or a batch HTTP publish, pnpr retries the request up to four times. Each retry checks
current permissions and package state again. Continued contention returns
`409 Conflict`.

Large publish requests also use the cache directory for retries. When too many
are in progress, another large publish returns `429 Too Many Requests`.

### Interruptions

- **Interrupted publishes**: A replica that stops between uploading the blobs and committing the metadata leaves blobs that no file maps, which are never served; [`pnpr gc`](cli.md#gc) removes them. A publish refused at its commit removes the incarnations it stored itself, unless another publish of the same bytes committed one of them in the meantime. It first records one a commit could still make live as gone, so that commit is refused rather than left pointing at nothing.
- **Local staging files**: A publish that times out, disconnects, or fails removes its `<cache>/.staging/` file when it stops. Unclean exits leave files that startup, periodic background sweeps, and [`pnpr cache-gc`](cli.md#cache-gc) reclaim once older than [`requestTimeout`](configuration.md#requesttimeout) (`1h` when disabled).
- **Multipart uploads**: If a large upload is aborted or interrupted, S3 retains incomplete multipart upload parts until explicitly aborted or cleaned up by policy.

### Operational recommendations

- **Abort incomplete multipart uploads**: Configure a lifecycle rule on your S3 bucket to abort incomplete multipart uploads after 1 to 7 days.
- **Bucket versioning**: Versioning can protect against accidental deletion. If enabled, configure a lifecycle rule to expire noncurrent object versions after your recovery window and remove expired delete markers. `pnpr gc` deletes current objects; their older versions keep using space until the lifecycle rule expires them.
- **Run periodic integrity audits**: Use `pnpr verify` to audit your hosted packages against stored S3 blobs, and `pnpr gc` to remove the archives no file maps. Both run while the registry serves.
- **Backup procedures**: Follow the guidelines in [Backup and restore](backup-and-restore.md) for snapshotting and restoring the database and hosted objects.
