---
id: container-images
title: Container images
---

Added in: v0.1.0-alpha.11

pnpr serves the OCI distribution API, so `docker`, `podman`, and `skopeo` can
push to and pull from the same server that hosts your packages.

```yaml
registries:
  images:
    type: hosted
    ecosystem: oci
    org: images
    packages:
      'acme/*': {}

defaultRegistry: images
```

```sh
printf '%s' "$PNPR_TOKEN" | docker login pnpr.example.com -u alice --password-stdin
docker push pnpr.example.com/acme/app:1.0
docker pull pnpr.example.com/acme/app:1.0
skopeo copy docker://pnpr.example.com/acme/app:1.0 oci:./app:1.0
```

Use a pnpr token as the password. The username is not checked, as on every
registry that takes a personal access token.

## Why `/v2/` is at the root

A client derives the API root from the image reference's host, so
`pnpr.example.com/acme/app:1.0` always requests
`/v2/acme/app/manifests/1.0`. `/v2/` therefore answers at the host root
whatever else pnpr serves, and this is the one ecosystem that cannot be moved
under a prefix. There is no reserved path segment: the image name you push is
the image name, and the repository name alone selects the registry through the
same declared-provenance rules every other surface uses.

`/oci/v2/` and `/oci/~<name>/v2/` answer as well, for `podman` and
`containerd`, whose `registries.conf` `location` and `hosts.toml` `server` both
accept a path.

A `packages:` key is either one repository name or `<namespace>/*`, which claims
every repository under one leading path component. The namespace is a single
component, so two namespace claims are either identical or disjoint. Repository
names follow the distribution spec's grammar and are folded to lowercase, so
`ACME/App` and `acme/app` are one repository rather than two directories on a
case-insensitive filesystem.

## Authentication

`GET /v2/` answers `401` with a `Basic` challenge to an anonymous caller even
where reads are open. A client settles its authentication scheme on that one
response, so a `200` would leave it with no way to authenticate a later push. A
client with no credentials carries on regardless, and a repository that admits
anonymous reads still answers its later requests.

Set `oci.bearerAuth: true` to offer a Bearer challenge instead. Clients then
exchange their pnpr credential at `/v2/token` for a five-minute token scoped to
`pull`, `push`, or `delete` on one repository. The endpoint grants only the
actions the caller already has; the parent credential's revocation, read-only
flag, and network restrictions all still apply. Anonymous tokens can pull public
repositories. Scoped tokens reach neither the other pnpr APIs nor the whole
catalog. Use the same [`secret:`](configuration.md#secret) and registry
configuration on every replica so their tokens interoperate.

## Uploads and limits

Blobs are uploaded in one request or in chunks, streamed to disk rather than
held in memory, and verified against the digest the client promised before
anything is stored.

```yaml
oci:
  maxBlobBytes: 10737418240   # 10 GiB, the default
  maxManifestBytes: 4194304   # 4 MiB, the default
```

The blob limit applies to a whole resumable upload, mounted layers included, and
also bounds an uncached upstream download.

The manifest write is the point a release becomes visible. It goes through the
same publish journal as every other ecosystem, so it either lands whole or
leaves nothing behind. A blob no manifest references is invisible rather than
half-published, which also means unreferenced blobs accumulate until they are
collected.

## What the API supports

* Ranged blob downloads on filesystem and S3 storage, including open-ended and
  suffix ranges. Responses carry `Accept-Ranges` and an ETag; `If-Range` resumes
  a matching blob or returns the whole one. Unsupported or malformed ranges are
  ignored.
* Cross-repository blob mounts, reusing a layer from a repository the caller may
  read. The caller also needs publish permission on the destination. A missing or
  inaccessible source starts an ordinary upload. Mounts stream through local
  scratch for digest verification rather than asking the client to resend.
* `GET /v2/<name>/referrers/<digest>`, returning an image index of the manifests
  and indexes attached to that subject, with the `artifactType` filter and
  annotations. A push acknowledges its subject with `OCI-Subject`. Follow the
  `Link` header to collect every page, even when a filtered page is empty. Each
  page reads at most 32 manifests and 8 MiB of manifest content.
* Pagination on `tags/list` and `_catalog` through `n` and `last`, with a `Link`
  header when another page follows. `last` is an exclusive lexical cursor, and
  `n=0` returns an empty list with no continuation link.

## Uploads across replicas

With S3 storage, upload sessions and their accepted chunks live in the bucket, so
a client can continue an upload on another replica that shares the storage prefix
and registry configuration. Each request stores only its new chunk. Completion
streams the accepted chunks to local scratch for digest verification, then
streams large blobs back to S3 in 8 MiB parts, so allow scratch space for
concurrent requests and completed layers.

Concurrent changes to one session are rejected; a client can query the session's
current offset and retry. A session accepts up to 10,000 nonempty chunks.
Completion freezes the chunk list and digest before promotion; if promotion
fails, retry completion with that digest and an empty body. Frozen sessions
reject further chunks and cancellation, and expire after 24 hours of inactivity.

Startup removes upload sessions idle for more than 24 hours, chunks included. On
a long-running deployment, restart rolling to reclaim abandoned uploads, and
configure the bucket's lifecycle policy to abort incomplete multipart uploads as
well, since a process killed mid-request cannot send its abort.

Filesystem storage keeps upload sessions local, so every request for one session
must reach the same instance.

## Reclaiming space

To collect blobs that no retained manifest references, stop **every writer**
sharing the store and run:

```sh
pnpr -c config.yaml oci-gc --registry images --dry-run
pnpr -c config.yaml oci-gc --registry images
```

`--registry` takes the concrete hosted OCI registry's config name. The command
recovers the local publish journal before scanning, so recover journals on every
replica's scratch volume before collecting shared storage, and keep writers
stopped for the whole scan and deletion. `--dry-run` reports candidates without
deleting them. `--min-age-secs` defaults to `86400`, which preserves recent
uploads so an interrupted push can resume; set it to zero to collect every
unreferenced blob.

Collection keeps untagged manifests, the children of retained indexes, and their
config and layer blobs, so removing a tag alone does not make its image
collectible. A missing or corrupt retained manifest stops collection before any
blob is deleted. The inventory covers unpublished repositories and nested
repository names, and is streamed into a temporary SQLite database, so local
temporary storage needs room for it.

`DELETE /v2/<name>/blobs/<digest>` removes an unreferenced blob for a caller with
`unpublish` permission, which the registry default denies. A blob reachable from
any retained manifest or index is refused. A stored deletion marker blocks
concurrent manifest publication, and a generation counter rejects publishes
staged before the deletion. If a deletion is interrupted, stop all writers and
run `oci-gc` without `--dry-run` to finish it and unblock the repository;
recovering an explicit deletion ignores `--min-age-secs`. Every replica sharing a
store must run this version before online deletion is used.

## Caching Docker Hub and GHCR

```yaml
registries:
  dockerhub:
    type: upstream
    ecosystem: oci
    url: https://registry-1.docker.io/
    public: true

defaultRegistry: dockerhub
```

An official Hub image is then pulled as `pnpr.example.com/library/alpine:latest`.
For GHCR, use `https://ghcr.io/` and the full repository name. Private origins
use the ordinary upstream [`auth:`](configuration.md#upstream-registries)
configuration and access rules.

pnpr negotiates repository-scoped pull tokens and restricts layer redirects to
the origin's known CDN hosts, so configured credentials never travel to a layer
CDN. A custom origin may use a token endpoint and downloads on its own origin.

Manifest bodies are verified before caching. Blob bodies are verified while
streaming and cached only after a successful digest check; a mismatch aborts the
response stream, and clients still verify the bytes they received. Tags refresh
on the configured upstream cache lifetime, and digest-addressed content is
immutable. `cache: false` disables the cache.

Upstream writes, catalog enumeration, tag listing, and referrer discovery are not
proxied.

## Publishing a manifest in a batch

An OCI entry in the [cross-ecosystem publish](ecosystems.md#one-publish-for-a-workspace-that-spans-ecosystems)
publishes a manifest whose layers have already been uploaded:

```json
{
  "ecosystem": "oci",
  "name": "acme/app",
  "reference": "1.0",
  "manifest": "<base64-encoded manifest bytes>"
}
```

`contentType` is optional when the manifest carries its own media type. An
index's child manifests must already be published. The batch body keeps the
endpoint's size limit, so upload large layers through the streaming `/v2/` API
first.
