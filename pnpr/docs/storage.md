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

## Layout

The hosted store has the same layout on disk and in a bucket:

```text
<storage>/
  layout.json                     marks the layout version
  default/packages/<name>/        registries without an org
  orgs/<org>/packages/<name>/     one namespace per org
<namespace>/packages/<name>/document.json
<namespace>/packages/<name>/blobs/<file>
```

A hosted registry declared under an
[ecosystem group](ecosystems.md#reusing-a-name-across-ecosystems) without an
`org` uses the org `<ecosystem>~<name>`, stored as `orgs/<ecosystem>!7e<name>/`,
for example `orgs/npm!7einternal/`.

Every package name, org and file name is one path segment. Lowercase
letters, digits, `.`, `_` and `-` are kept, and every other byte is written
as `!` and two hex digits, so `@scope/pkg` is stored as `!40scope!2fpkg`.

pnpr refuses to start on a store written by a newer pnpr, or on one in the
layout pnpr used before `layout.json` existed. Convert such a store with
[`pnpr upgrade-storage`](cli.md#upgrade-storage). A cache in another layout is
emptied on startup and refilled on demand.

## Backends

By default both are local directories. Adding an `s3:` block moves the
**hosted** store and the enabled **shared-artifact** store into an S3-compatible
object store, so they can be shared by several stateless pnpr replicas. The
ordinary upstream and resolver cache stays on local disk.

Shared artifacts use a reserved `.pnpr-artifacts/v0/` namespace under the
configured prefix; hosted package objects keep their normal namespace. Without
S3, artifacts live under `<cache>/shared-artifacts/v0`. They are rebuildable
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
- **Tarballs**: finalizing a tarball is compare-and-swap. A byte-identical
  object is tolerated, but an object with different bytes left by a concurrent
  publisher of the same `name@version` is never overwritten; the losing publish
  gets a `409` before it writes any packument.
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

## S3 publish journal and lifecycle caveats

When publishing packages to an S3 backend, pnpr coordinates uploads through a local write-ahead commit journal:

1. **Local staging**: Incoming blobs are staged under `<cache>/pnpr-hosted-staging/`.
2. **Journal seal**: pnpr writes the transaction under `<cache>/.pnpr-journal/<txn>/` and seals it before applying object-store writes.
3. **Apply**: pnpr uploads the blobs and conditionally updates the packument.
4. **Journal cleanup**: The local journal entry is removed once the object store confirms the write.

### Multi-replica considerations

The write-ahead journal is kept on the local disk of the replica handling the publish request. Keep these operational details in mind:

- **Startup recovery**: If a replica terminates unexpectedly mid-publish, it automatically recovers pending journal entries in its `<cache>/.pnpr-journal/` on next startup, applying sealed transactions and rolling back unsealed ones.
- **Ephemeral disks**: In containerized multi-replica environments where instances use ephemeral local storage, destroying a failed instance before it restarts means its local journal cannot be replayed. Any partially uploaded blobs in S3 remain as unreferenced objects.
- **Multipart uploads**: If a large upload is aborted or interrupted, S3 retains incomplete multipart upload parts until explicitly aborted or cleaned up by policy.

### Operational recommendations

- **Abort incomplete multipart uploads**: Configure a lifecycle rule on your S3 bucket to abort incomplete multipart uploads after 1 to 7 days.
- **Enable bucket versioning**: Enable S3 bucket versioning to protect against accidental object deletion or overwrite.
- **Run periodic integrity audits**: Use `pnpr verify` to audit your hosted packages against stored S3 blobs, and `pnpr verify --prune-orphans` to detect and remove orphaned blobs associated with verified npm packages.
- **Backup procedures**: Follow the guidelines in [Backup and restore](backup-and-restore.md) for snapshotting and restoring S3 package data and revision history.

