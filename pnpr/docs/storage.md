---
id: storage
title: Storage backends (S3 / R2)
---

pnpr keeps two kinds of data:

- **Hosted** — the source of truth: packages published to this server plus
  anything served in static mode. Lives under `storage`. Packages awaiting
  approval through [staged publishing](endpoints.md#staged-publishing-endpoints)
  are held here too, under a reserved `.staged/` namespace.
- **Cache** — the disposable mirror of upstream registries plus the resolver
  cache, lockfile-verdict cache, local shared-artifact store, and S3 upload
  staging scratch. Lives under `cache` (defaults to
  `<storage>/.pnpr-cache`).

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
**MinIO**, **Backblaze B2**, **Wasabi**, etc. — point `endpoint` at the right
host.

```yaml
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
| `forcePathStyle` | no | Use path-style addressing (`endpoint/bucket/key`) instead of virtual-hosted (`bucket.endpoint/key`). MinIO typically needs `true`; AWS and R2 work with the default. |
| `allowHttp` | no | Allow plain-HTTP endpoints — needed for a local MinIO over `http://`. Defaults to HTTPS-only. |

## Concurrent writers {#concurrent-writers}

Several stateless pnpr replicas can share one bucket. Because a replica's
in-process package lock only serializes that replica, writes to the hosted
store are made safe across replicas at the object-store level rather than by
locking:

- **Packument updates** — publish, partial unpublish, and dist-tag writes read
  the packument together with its object-store version and write back
  conditionally, so an update computed from a stale copy is rejected instead of
  overwriting a newer one. Dist-tag writes retry on conflict, since their
  mutation can simply be replayed against the fresh packument. A write that
  keeps losing surfaces as an HTTP `409` rather than silently discarding
  another writer's update.
- **Tarballs** — finalizing a tarball is compare-and-swap. A byte-identical
  object is tolerated, but an object with different bytes left by a concurrent
  publisher of the same `name@version` is never overwritten; the losing publish
  gets a `409` before it writes any packument.
- **Shared artifacts** — immutable artifact objects use create-only writes, and
  the quota counter uses conditional updates across replicas. A publication
  whose compatibility set overlaps an existing artifact for the same input key
  gets a `409`; a byte-identical retry succeeds. After an ambiguous object-store
  write failure, pnpr reclaims blobs referenced by no stored envelope and
  rebuilds quota once active publications drain.

This applies to the S3 backend. The local filesystem backend keeps its
single-process behavior, since the shared-store race is specific to replicas
sharing one object store.

## A complete Cloudflare R2 example

```yaml
# pnpr.yaml
storage: ./storage

s3:
  bucket: my-pnpr-packages
  region: auto
  endpoint: https://abc123def456.r2.cloudflarestorage.com

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

MinIO over `http://` needs `forcePathStyle` and `allowHttp`:

```yaml
s3:
  bucket: pnpr
  region: us-east-1
  endpoint: http://127.0.0.1:9000
  forcePathStyle: true
  allowHttp: true
  accessKeyId: minioadmin
  secretAccessKey: minioadmin
```
