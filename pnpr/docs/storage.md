---
id: storage
title: Storage backends (S3 / R2)
---

pnpr keeps two kinds of data:

- **Hosted**: the source of truth. It holds packages published to this server plus
  anything served in static mode. Lives under `storage`. Packages awaiting
  approval through [staged publishing](endpoints.md#staged-publishing-endpoints)
  are held here too, under a reserved `.staged/` namespace.
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
  layout.json                     marks the layout version
  default/<ecosystem>/            registries without an org
  orgs/<org>/<ecosystem>/         one namespace per org and ecosystem
<namespace>/archives/<shard>/<identity>.<incarnation>
```

`<ecosystem>` is `npm`, `cargo`, `pypi`, or `oci`. A hosted registry declared
under an [ecosystem group](ecosystems.md#reusing-a-name-across-ecosystems)
without an `org` uses its own registry name as its `org`, under
`orgs/<name>/<ecosystem>/`, for example `orgs/internal/npm/`. In an org name,
lowercase letters, digits, `.`, `_` and `-` are kept. Every other byte is
written as `!` and two hex digits.

A namespace stores the bytes of an archive once, by their identity: the digest
its ecosystem checks them by, as `<algorithm>-<hex>`. That is the `sha512` of
npm's `dist.integrity`, and the `sha256` of an image blob's digest, a crate's
checksum, or a Python distribution's hash. `<shard>` is the first two hex
digits of the digest, so `sha512-ab12…` is under `archives/ab/`. A public file
name, such as `left-pad-1.0.0.tgz`, never reaches the store: the
[metadata](#sqlite-metadata) maps each file of a package to the identity of
its bytes, and a download of the file reads the bytes through that mapping.
Files with the same bytes, of one package or of several, share one stored
archive.

`<incarnation>` counts the times the bytes were stored, from `1`. Bytes a
[reclaim](cli.md#gc) removed and a later publish stores again take the next
incarnation, so a key is never written again once a reclaim may remove it.

pnpr refuses to start on a store written by a newer pnpr, or on one in an
earlier layout, including the one pnpr used before `layout.json` existed: move
to a fresh storage directory or bucket prefix, wipe pnpr's metadata tables, and
republish. A cache in another layout is emptied on startup and refilled on
demand.

## Metadata in SQLite {#sqlite-metadata}

pnpr keeps the metadata of every hosted package in an embedded SQLite
database, by default `<storage>/.metadata/metadata.sqlite`. The `metadata`
block names another database by URL:

```yaml title="pnpr.yaml"
storage: ./storage
metadata:
  # Optional. A relative path is resolved against the config file's directory.
  url: sqlite:./metadata.sqlite
```

Archives stay in the hosted store. The database holds every hosted package's
metadata in every ecosystem. pnpr keeps metadata only in a database: an `s3:`
block of a registry that hosts packages needs a [libSQL](#libsql-metadata)
`url`, and pnpr refuses to start on a
`metadata` block that still names a `backend`. Earlier releases could keep
each package's metadata as a document beside its archives. pnpr does not read
those documents. A filesystem store from then starts with empty metadata. An
`s3:` store from then is refused at startup until `metadata.url` names a
libSQL database, and then starts with empty metadata. Either way, republish
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
  unpublished package, and refuses to accept one again with `403`, even when
  the package's tombstone is gone.
- A dist-tag change rewrites only that tag, and pnpr keeps the history of
  every move of a tag: when it moved, from which version to which, and who
  moved it. A publish, a document update, and a dist-tag change each record
  the user who made them.
- Every change to an org's packages advances that org's generation. A hosted
  npm package response, and the response to a dist-tag change, carry it in a
  `pnpr-generation` header as `<org>:<generation>`, with an empty org for
  registries without one. Generations of different orgs are unrelated.
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
combined with an `s3:` block. Back the database file up together with the
hosted store.

### Schema changes {#metadata-schema-changes}

pnpr creates its tables in an empty database and marks the database with
the version of their schema, in the `schema_version` table. pnpr does not
migrate the metadata schema before 1.0. Until then the version is `0.x`, and
a pnpr release that changes the schema bumps its minor part. That pnpr
refuses a database that holds another version, before it changes anything in
it. The error names the version pnpr expects and what the database holds:
another version, the integer schema version an older pnpr stored, pnpr's
metadata tables without a version, or pnpr's version without some of its
tables, as a wipe that kept the `schema_version` table leaves it.

To start that pnpr, wipe pnpr's metadata tables by hand: drop the
`schema_version` table and the metadata tables beside it. pnpr then creates
them afresh. Leave the tables of the [auth backend](auth-backends.md) in
place when it shares the database. A SQLite database that holds only the
metadata, as the default `<storage>/.metadata/metadata.sqlite` does, can be
deleted instead, with its `-wal` and `-shm` files.

## Metadata in libSQL {#libsql-metadata}

Several pnpr instances share one metadata database when its URL names a
libSQL server, such as [Turso](https://turso.tech) or a self-hosted `sqld`.
Every behavior of [SQLite metadata](#sqlite-metadata) applies. A libSQL URL
works with an `s3:` block, so archives stay in the bucket:

```yaml title="pnpr.yaml"
s3:
  bucket: my-pnpr-packages
metadata:
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
- An instance always sees its own changes.
- Permission checks, the reads a write depends on, and the response to a
  retried publish read the primary.
- When the replica cannot sync, pnpr reads the primary.

Two instances publishing to one package at the same moment both land. The
publish that commits second finds the package at another generation, rebases
its versions, tags, and other changes onto what the first committed, and
commits again, as if the two had run one after the other. A version, tag, or
time the first added stays; any other field the second changed is taken whole.
When the package's maintainers changed meanwhile, for example because another
user created the package first, the second publish is refused with `409`
instead.

## Backends

By default both are local directories. Adding an `s3:` block moves the
**hosted** store and the enabled **shared-artifact** store into an S3-compatible
object store, so they can be shared by several stateless pnpr replicas. Their
metadata then lives in a [libSQL database](#libsql-metadata) the replicas
share. The ordinary upstream and resolver cache stays on local disk.

Shared artifacts use a reserved `.artifacts/` namespace under the
configured prefix; hosted package objects keep their normal namespace. Without
S3, artifacts live under `<cache>/shared-artifacts`. They are rebuildable
cache data rather than authoritative packages, but publication slots are
immutable while they exist.

Because any S3-compatible endpoint works, this also covers **Cloudflare R2**,
**MinIO**, **Backblaze B2**, **Wasabi**, and so on. Point `endpoint` at the right
host.

```yaml title="pnpr.yaml"
storage: ./storage
# cache: ./cache     # local proxy cache + resolver cache + S3 upload staging

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

metadata:
  # Package metadata of a hosted registry on S3 lives in a libSQL database.
  url: libsql://<your-db>.turso.io
  authToken: ${PNPR_LIBSQL_TOKEN}
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

Several stateless pnpr replicas can share one bucket. Because a replica's
in-process package lock only serializes that replica, writes to the hosted
store are made safe across replicas at the object-store level rather than by
locking:

- **Packument updates**: publish, partial unpublish, and dist-tag writes read
  the packument together with its object-store version and write back
  conditionally, so an update computed from a stale copy is rejected instead of
  overwriting a newer one. Dist-tag writes retry on conflict, since their
  mutation can be replayed against the fresh packument. A write that
  keeps losing surfaces as an HTTP `409` rather than silently discarding
  another writer's update.
- **Archives**: an archive is stored under the identity of its bytes and an
  incarnation the metadata database counts (see [Layout](#layout)), so two
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
storage: ./storage

s3:
  bucket: my-pnpr-packages
  region: auto
  endpoint: https://abc123def456.r2.cloudflarestorage.com

metadata:
  url: libsql://my-pnpr.turso.io
  authToken: ${PNPR_LIBSQL_TOKEN}

auth:
  htpasswd:
    file: ./htpasswd

registries:
  local:
    type: hosted
    packages:
      '@mycompany/*':
        publish: $authenticated

  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true

  main:
    type: router
    sources: [local, npmjs]

defaultRegistry: main

artifacts:
  enabled: true
```

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
3. **Commits the metadata**: pnpr commits every package of the publish to the metadata database in one transaction.

### Interruptions

- **Interrupted publishes**: A replica that stops between uploading the blobs and committing the metadata leaves blobs that no file maps, which are never served; [`pnpr gc`](cli.md#gc) removes them. A publish refused at its commit removes the incarnations it stored itself, unless another publish of the same bytes committed one of them in the meantime. It first records one a commit could still make live as gone, so that commit is refused rather than left pointing at nothing.
- **Multipart uploads**: If a large upload is aborted or interrupted, S3 retains incomplete multipart upload parts until explicitly aborted or cleaned up by policy.

### Operational recommendations

- **Abort incomplete multipart uploads**: Configure a lifecycle rule on your S3 bucket to abort incomplete multipart uploads after 1 to 7 days.
- **Enable bucket versioning**: Enable S3 bucket versioning to protect against accidental object deletion or overwrite.
- **Run periodic integrity audits**: Use `pnpr verify` to audit your hosted packages against stored S3 blobs, and `pnpr gc` to remove the archives no file maps. Both run while the registry serves.
- **Backup procedures**: Follow the guidelines in [Backup and restore](backup-and-restore.md) for snapshotting and restoring S3 package data and revision history.

