# pnpr

A pnpm-compatible npm registry server, written in Rust.

Lives in the [pnpm monorepo](https://github.com/pnpm/pnpm) under `registry/`.

## Registry directory

`GET /-/pnpr/v0/registries` lists the named registries visible to the caller.
It returns `registries`, the visible `defaultRegistries` keyed by ecosystem, and the
mounted `ecosystems` with their `available` and `prefixed` flags.

Each entry contains its `name`, `kind` (`hosted`, `upstream`, or `router`),
and `ecosystem`. The identity is `(ecosystem, name)`. Concrete registries report namespace `patterns`;
routers report ordered `sources`. These fields are `null` when their details
cannot be disclosed under the caller's access rules. Upstream addresses,
credentials, storage paths, and package access rules are never returned.
Responses are private and must not be cached.

An explicit `/<ecosystem>/~name` chooses a registry; the path without a name uses
that ecosystem's configured default. A single-ecosystem server omits the ecosystem prefix. Within an ecosystem, a router selects the first source
whose namespace claims the package. A missing package or failed upstream is
final, without fallback to another source. Package access rules still apply.

## Ecosystem-scoped registries

Group registries by ecosystem to reuse a name across protocols:

```yaml
registries:
  npm:
    internal:
      type: hosted
      packages:
        '@example/*': {}
    public:
      type: upstream
      url: https://registry.npmjs.org/
      public: true
    main:
      type: router
      sources: [internal, public]
  cargo:
    internal:
      type: hosted
    main:
      type: router
      sources: [internal]
defaultRegistry:
  npm: main
  cargo: main
```

Here `/npm/~internal` and `/cargo/~internal` are independent registries. Router
sources resolve within their ecosystem group, and each ecosystem has its own
default. Source references cannot cross ecosystem groups. Access rules, teams,
upstream credentials, and caches belong to the individual registry.

A grouped hosted registry defaults to the storage namespace `<ecosystem>~<name>`.
Set `org` explicitly to use an existing namespace when moving a flat registry
into a group, including `org: ''` for the flat storage root. Two hosted registries
cannot share a storage namespace. The existing flat configuration format and
its shared `defaultRegistry: main` remain supported.

## OpenID Connect

[Configure OIDC sign-in and keyless CI publishing](./OIDC.md) with Okta,
Microsoft Entra ID, Google Workspace, or GitHub Actions workload identity.

## Cargo compilation cache

pnpr's experimental artifact service supports sccache's WebDAV backend.
Cargo compilation can use a local disk cache and share a remote cache between
CI and developer machines. No pnpm workspace or `package.json` is required.

Declare each cache and its permitted readers and publishers in pnpr's YAML:

```yaml
artifacts:
  enabled: true
  compilerCaches:
    acme:
      access: [ci-builder, alice, bob]
      publish: ci-builder
```

These are pnpr account names. Each caller supplies its own pnpr token.
`access` is required for both reads and writes; `publish` additionally gates
writes. An empty list denies access. The usual `$authenticated` access token
is also supported. Undeclared caches are unavailable. This policy lets CI
publish while developers only read, even when their tokens otherwise permit
registry writes. Read-only token restrictions are also enforced.

Install [sccache 0.17.0 or newer](https://github.com/mozilla/sccache) with
WebDAV support (`sccache --help` lists enabled backends). Configure the
environment before starting sccache or invoking Cargo:

```sh
export RUSTC_WRAPPER=sccache
export CARGO_INCREMENTAL=0
export SCCACHE_MULTILEVEL_CHAIN=disk,webdav
export SCCACHE_WEBDAV_ENDPOINT=https://cache.example.com/-/pnpr/v0/compiler-cache/acme
export SCCACHE_WEBDAV_TOKEN="$PNPR_TOKEN"
export SCCACHE_WEBDAV_RW_MODE=READ_ONLY

cargo build
sccache --show-stats
```

`PNPR_TOKEN` must contain that caller's token;
keep it in developer credentials or CI secrets. CI uses its builder account's
token and sets `SCCACHE_WEBDAV_RW_MODE=READ_WRITE`. The pnpr policy enforces
publication permission independently of this client setting.

`SCCACHE_MULTILEVEL_CHAIN=disk,webdav` checks disk first, then pnpr, and
backfills disk on a remote hit. `SCCACHE_DIR` and `SCCACHE_CACHE_SIZE` configure
the local cache. Remote write failures do not fail a successful disk write
with sccache's default multilevel write-error policy. A running sccache daemon
retains its startup configuration; restart it with `sccache --stop-server`
after changing these settings. Use separate daemons when concurrently building
projects that need different cache settings.

Reuse depends on sccache's compilation keys, including the compiler, target,
features, flags, source inputs, and dependencies. Rust keys in sccache 0.17.0
also include the absolute compilation directory. CI and developer builds need
matching source paths, for example a common `/workspace` mount inside their
build containers, including consistent Cargo registry source paths.
`SCCACHE_BASEDIRS` does not remove this Rust restriction. Align toolchains
and build profiles for useful hit rates. Rust incremental compilation must
be disabled. System linking is not cached, and procedural macros with
undeclared filesystem inputs have correctness limitations. See
[sccache's Rust support](https://github.com/mozilla/sccache/blob/v0.17.0/docs/Rust.md).

sccache 0.17.0 also treats a multilevel cache as read-only when any tier is
read-only. Developers can read shared entries and those hits backfill disk,
but newly compiled misses are not added to disk in this mode. Builds still
succeed. For caching new local compilations, use a disk-only daemon or a
cache for which the developer has publication access. This is an upstream
[multilevel-cache limitation](https://github.com/mozilla/sccache/blob/v0.17.0/src/cache/multilevel.rs).

The WebDAV endpoint trusts pnpr and the accounts allowed to publish. Use HTTPS
outside localhost and grant publication only to trusted builds, keeping
untrusted pull-request jobs out of the shared writer credentials. Stock
sccache does not verify pnpm's signed side-effects envelopes. pnpr verifies
stored compiler entries against a digest of their cache scope, key, and bytes
before serving them; this detects storage corruption, not a malicious server
or authorized publisher.

Entries use the same configured filesystem or S3 artifact store and quota
accounting as side effects, in a separate compiler-cache namespace. A cache
key is immutable: the first successful PUT wins. `SCCACHE_WEBDAV_KEY_PREFIX`
can select a fresh namespace when needed; this does not reclaim old entries.
The experimental limits are 256 MiB per compiler entry, 1 GiB per owner/cache
name, and 10 GiB globally across artifacts. A compiler cache sharing a name
with a side-effects owner also shares its quota. There is no automatic
eviction of live compiler entries yet.

Each server accepts at most two concurrent compiler uploads, acquiring capacity
before reading request bodies. Additional uploads receive HTTP 503 with
`Retry-After: 1`. HEAD requests inspect object metadata without downloading the
entry; GET requests always verify the full content digest.

The endpoint implements GET, HEAD, and PUT at
`/-/pnpr/v0/compiler-cache/<cache>/<key>`, plus PROPFIND for virtual parent
directories. It is the subset used by sccache,
not a general WebDAV filesystem. Local builds continue to use ordinary
Cargo commands. The `sideEffectsCache` setting controls npm dependency side
effects and does not configure this integration.

## Browser registry UIs

Cross-origin browser access and upstream discovery are disabled by default. To
run a registry UI on a separate origin, list that exact origin and opt the
needed upstreams into search and organization discovery:

```yaml
cors:
  allowedOrigins:
    - https://registry-ui.example.com

registries:
  local:
    type: hosted
    access: $all
    packages:
      '@example/*': {}
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
    search: true
  main:
    type: router
    sources: [local, npmjs]

defaultRegistry: main
```

Origins must contain only an `http` or `https` scheme, host, and optional port.
The `search` setting also enables `/-/org/{scope}/package` discovery for that
upstream. pnpr applies registry routing and access rules to returned entries and
uses only the upstream credentials from its configuration, never a browser
caller's authorization header. Discovery refuses redirects and sends configured
upstream headers only over HTTPS or loopback HTTP. Search totals count only
visible, deduplicated results and are exact across every participating source.
Hosted npm packages and Cargo crates can be browsed without a search term:

```text
GET /-/v1/search?browse=true&size=20&from=0
GET /cargo/api/v1/crates?browse=true&per_page=20&page=1
```

Use the ecosystem prefix and named registry mount appropriate for your server.
Browse results follow registry routing and access rules, exclude upstreams and
staged publications, and use the same response format and pagination limits as
search. An empty search without `browse=true` still returns no results.

To bound work from a single browser request, pnpr rejects upstream searches
that would scan more than 2,000 upstream results or eight upstream pages.
Offsets that would require a larger upstream scan are also rejected. Refine the
search term when a query reaches that limit.

## Cargo and Python registries

pnpr serves npm, Cargo, Python, and image registries from one instance. When
more than one ecosystem is configured, `/<ecosystem>/~<name>/` addresses a
registry and `/<ecosystem>/` the default registry, with `npm`, `cargo`,
`pypi`, and `oci` as the codes. When only one ecosystem is configured, its
prefix is omitted and clients use the host root or `/~<name>/`. Image
registries are the exception described under "Image registries" below: their
`/v2/` endpoints answer at the host root whichever else is configured.

The endpoints that belong to no single ecosystem stay at the root: `/-/ping`,
the `/-/pnpr/v0/` protocols (resolve, verify-lockfile, shared artifacts, and
the cross-ecosystem publish transaction), and the account endpoints that mint
and manage tokens.

A hosted or upstream registry declares the ecosystem it serves (`ecosystem:`,
npm by default). A router may list sources of every ecosystem: a request only
ever sees the sources that speak its protocol, so one router can be the
default target for all of them.

```yaml
registries:
  local:
    type: hosted
    packages:
      '@example/*': {}
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  crates:
    type: hosted
    ecosystem: cargo
    org: crates
    packages:
      my-crate: {}
  crates-io:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
  python:
    type: hosted
    ecosystem: pypi
    org: python
    packages:
      my-package: {}
  pypi-org:
    type: upstream
    ecosystem: pypi
    url: https://pypi.org/simple/
    public: true
  main:
    type: router
    sources: [local, npmjs, crates, crates-io, python, pypi-org]

defaultRegistry: main
```

A **Cargo** registry serves a sparse index at `/cargo/index/` and the crates
API at `/cargo/api/v1/crates/` (or `/cargo/~<name>/...` for a named
registry). Point `cargo` at it with:

```toml
# .cargo/config.toml
[registries.pnpr]
index = "sparse+https://pnpr.example.com/cargo/index/"
```

`cargo publish --registry pnpr`, `cargo yank --registry pnpr`, and
dependencies with `registry = "pnpr"` then go through pnpr. The `config.json`
pnpr serves points downloads back at itself, so proxied crates are cached and
verified against the upstream index checksum. Use a pnpr token as the registry
token; `cargo` sends it as a bare `Authorization` header and pnpr accepts that
alongside `Bearer`. A registry that is not anonymously readable advertises
`auth-required`, so `cargo` sends the token on index and download requests
too. Crate names are compared case-insensitively, and exact `packages:` keys
must be valid crate names.

A **Python** registry serves the Simple Repository API at `/pypi/simple/`
(PEP 503 HTML and PEP 691 JSON, chosen by `Accept`) and files at
`/pypi/files/<project>/<filename>`, and accepts uploads on the legacy API at
`/pypi/legacy/` (or `/pypi/~<name>/...` for a named registry):

```sh
pip install --index-url https://pnpr.example.com/pypi/simple/ my-package
twine upload --repository-url https://pnpr.example.com/pypi/legacy/ dist/*
```

Use `__token__` as the username and a pnpr token as the password, the PyPI
convention. Project names are compared PEP 503 normalized, so `My_Package`,
`my.package`, and `my-package` are one project, and exact `packages:` keys are
normalized the same way. Proxied files are fetched from the URL the upstream
page lists, verified against its `sha256`, and cached. The project list at
`/pypi/simple/` enumerates hosted projects only. Upstream credentials are sent
only to the upstream's own origin, never to the separate host an index points
downloads at.

### One publish for a workspace that spans ecosystems

`PUT /-/pnpr/v0/publish` publishes packages of any ecosystem in a single
transaction. Each entry names its `ecosystem` — absent means npm, so the body
the npm batch endpoint takes is already a valid one — and carries what that
ecosystem's own publish endpoint takes, with the binary parts base64-encoded:

```json
{
  "packages": [
    { "name": "@acme/ui", "versions": {}, "_attachments": {} },
    { "ecosystem": "cargo", "metadata": { "name": "acme", "vers": "0.1.0" },
      "archive": "<base64 .crate>" },
    { "ecosystem": "pypi", "name": "acme", "version": "0.1.0",
      "filetype": "bdist_wheel", "filename": "acme-0.1.0-py3-none-any.whl",
      "content": "<base64 wheel>" }
  ]
}
```

Every entry is authorized and verified before any of them is written, and the
write is one journaled transaction: the release either lands whole or leaves
nothing behind, and one interrupted by a crash is completed on the next
startup rather than staying half-published. A read that lands while the
transaction applies can still see some of the release and not the rest. If
another writer has already published one of the files, that package is left
out and reported with `409`, and the rest of the release stays: the bytes that
won the slot are someone else's published release. The endpoint sits outside the per-ecosystem prefixes because the
batch belongs to no single ecosystem; the npm-only `PUT /-/pnpm/v1/publish`
stays where it is. `GET /-/pnpr` advertises support as `publish: [0]`.

Cargo and Python proxies restrict downloads and redirects to their configured
upstream origin and the existing `routes.public` allowlist. The official
crates.io and PyPI upstreams also permit their respective download hosts,
`static.crates.io` and `files.pythonhosted.org`. For a custom registry using a
separate download host, declare it explicitly:

```yaml
routes:
  public:
    - registry: https://downloads.example.com/
```

Configured headers are sent only over HTTPS or loopback HTTP. Redirects rebuild headers for each destination, so configured credentials stay
on the upstream origin.

## Image registries

An image registry serves the OCI distribution API at `/v2/`, and unlike every
other ecosystem it cannot be moved under a prefix. A client derives the API
root from the image reference's host, so `pnpr.example.com/acme/app:1.0`
always requests `/v2/acme/app/manifests/1.0`. `/v2/` therefore answers at the
host root whatever else pnpr serves, and the repository name alone selects the
registry through the same declared-provenance rules the other surfaces use.
There is no reserved path segment, so the image name you push is the image
name. `/oci/v2/` and `/oci/~<name>/v2/` answer as well, for `podman` and
`containerd`, whose `registries.conf` `location` and `hosts.toml` `server`
both accept a path.

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

A `packages:` key is either one repository name or `<namespace>/*`, which
claims every repository under one leading path component. The namespace is a
single component, so two namespace claims are either the same or disjoint.
Repository names follow the distribution spec's grammar and are folded to
lowercase, so `ACME/App` and `acme/app` are one repository rather than two
directories on a case-insensitive filesystem.

Use a pnpr token as the password; the username is not checked, as on every
registry that takes a personal access token:

```sh
printf '%s' "$PNPR_TOKEN" | docker login pnpr.example.com -u alice --password-stdin
docker push pnpr.example.com/acme/app:1.0
docker pull pnpr.example.com/acme/app:1.0
skopeo copy docker://pnpr.example.com/acme/app:1.0 oci:./app:1.0
```

With S3 storage, upload sessions and their accepted chunks live in the bucket.
A client can continue an upload on another replica with the same storage prefix
and registry configuration. Each request stores only its new chunk. Completion
streams the accepted chunks to local scratch for digest verification, then
streams large blobs back to S3 in 8 MiB parts. Allow scratch space for concurrent
requests and completed layers. Concurrent changes to one session are rejected;
clients can query its current offset and retry. A session accepts up to 10,000
nonempty chunks. Completion freezes the chunk list and digest before promotion.
If promotion fails, retry completion with that digest and an empty request body.
Frozen sessions reject further chunks and cancellation, and expire after 24 hours
of inactivity.

Startup removes upload sessions idle for more than 24 hours, including their
accepted chunks. Run a rolling restart periodically if abandoned uploads need
reclaiming on a long-running deployment. Configure the bucket's lifecycle policy
to abort incomplete multipart uploads as well, since a process killed during a
multipart request cannot send its abort request. Filesystem storage keeps upload
sessions local and requires requests for a session to reach the same instance.

To reclaim old blobs that no retained manifest references, stop **every writer**
sharing the store and run:

```sh
pnpr -c config.yaml oci-gc --registry images --dry-run
pnpr -c config.yaml oci-gc --registry images
```

Use the concrete hosted OCI registry's config name for `--registry`. The command
recovers the local publish journal before scanning. Recover journals on every
replica's scratch volume before collecting shared storage. Keep writers stopped
for the entire scan and deletion. `--dry-run` reports candidates without deleting
them. `--min-age-secs` defaults to 86400, preserving recent uploads for interrupted
pushes to resume. Set it to zero to collect all unreferenced blobs.

Collection keeps untagged manifests, the children of retained indexes, and their
config and layer blobs. Removing a tag alone does not make its image collectible.
Missing or corrupt retained manifests stop collection before any blobs are
deleted. The inventory includes unpublished repositories and nested repository
names. Collection streams the inventory into a temporary SQLite database, so
local temporary storage must have room for the inventory metadata.

`DELETE /v2/<name>/blobs/<digest>` removes an unreferenced blob when the caller
has `unpublish` permission. A blob reachable from any retained manifest or index
is refused. A stored deletion marker blocks concurrent manifest publication,
and a generation counter rejects publishes staged before the deletion.
If deletion is interrupted, stop all writers and run `oci-gc` without `--dry-run`
to finish it and unblock the repository. Recovery of an explicit deletion ignores
`--min-age-secs`. All replicas sharing a store must run this version before using
online deletion.


`GET /v2/` answers `401` with a `Basic` challenge to an anonymous caller even
where reads are open. A client settles its authentication scheme on that one
response, so a `200` would leave it with no way to authenticate a later push.
A client with no credentials carries on regardless, and a repository that
admits anonymous reads still answers its later requests.

Blobs are uploaded in one request or in chunks, streamed to disk rather than
held in memory, and verified against the digest the client promised before
anything is stored. By default, a layer may be up to 10 GiB and a manifest up to
4 MiB. Configure positive byte limits with:

```yaml
oci:
  maxBlobBytes: 10737418240
  maxManifestBytes: 4194304
```

The blob limit applies to the entire resumable upload, including mounted layers.
It also bounds uncached upstream downloads.
The manifest write is the point a release becomes visible: it goes through the
same publish journal as every other ecosystem, so it either lands whole or
leaves nothing behind. A blob no manifest references is invisible rather than
half-published, which also means unreferenced blobs accumulate until they are
collected.

Blob downloads accept a single HTTP byte range on filesystem and S3 storage,
including open-ended and suffix ranges. Responses include `Accept-Ranges` and
an ETag. `If-Range` resumes a matching blob or returns the full blob when its
validator does not match. Unsupported or malformed ranges are ignored.

Cross-repository blob mounts reuse a layer from a repository the caller may
read. The caller also needs permission to publish to the destination. A missing
or inaccessible source starts an ordinary upload. Mounts stream through local
scratch for digest verification; they do not require the client to resend the layer.

`GET /v2/<name>/referrers/<digest>` returns an OCI image index of manifests and
indexes attached to that subject. It supports the `artifactType` filter and
includes annotations. A push acknowledges its subject with `OCI-Subject`.
Follow the `Link` header to collect all pages, even when a filtered page is empty.
Each page reads at most 32 manifests and 8 MiB of manifest content. Existing
manifests are indexed incrementally as those pages are queried. Removing a
manifest removes its referrer entry; removing a tag keeps it.

`tags/list` and `_catalog` accept `n` and `last` for pagination and return a
`Link` header when another page is available. `last` is an exclusive lexical
cursor. `n=0` returns an empty list without a continuation link.

The filesystem catalog uses a separate package index to find published repositories
nested beneath either published or blob-only parent paths. The blob-only parents
themselves remain absent until an operation creates their repository document.
Startup indexes legacy stores once. This initial scan visits existing files;
subsequent requests enumerate package names without walking layer files.

Set `oci.bearerAuth: true` to offer a Bearer challenge. Clients exchange their
pnpr credential at `/v2/token` for a five-minute token scoped to repository
`pull`, `push`, or `delete` actions. The endpoint grants only permitted actions.
The parent credential's revocation, read-only flag, and network restrictions
remain effective. Anonymous tokens can pull public repositories. Use the same
`secret:` and registry configuration on all replicas so their tokens interoperate.
Scoped tokens cannot access other pnpr APIs or the whole catalog.

An OCI batch entry publishes a manifest whose layers have already been uploaded:

```json
{
  "ecosystem": "oci",
  "name": "acme/app",
  "reference": "1.0",
  "manifest": "<base64-encoded manifest bytes>"
}
```

Include this entry alongside npm, Cargo, or Python entries in the `packages`
array of `PUT /-/pnpr/v0/publish`. `contentType` is optional when the manifest
contains its media type. An index's child manifests must already be published.
The batch body keeps the endpoint's existing size limit; upload large layers
through the streaming `/v2/` API first.

Docker Hub and GHCR can be configured as pull-through upstreams:

```yaml
registries:
  dockerhub:
    type: upstream
    ecosystem: oci
    url: https://registry-1.docker.io/
    public: true
defaultRegistry: dockerhub
```

Pull an official Hub image as `pnpr.example.com/library/alpine:latest`. For GHCR,
use `https://ghcr.io/` and the full repository name. Private origins use the
existing upstream `auth:` configuration and access rules. pnpr negotiates
repository-scoped pull tokens and restricts layer redirects to the origin's
known CDN hosts. Configured credentials never travel to a layer CDN. Custom
origins may use a token endpoint and downloads on their own origin.

Manifest bodies are verified before caching. Blob bodies are verified while
streaming and are cached only after a successful digest check. A mismatch aborts
the response stream. Clients still verify the streamed bytes. Tags refresh according to the configured upstream cache lifetime;
digest-addressed content is immutable. `cache: false` disables this cache.
Upstream writes, catalog enumeration, tag listing, and referrer discovery are
not proxied. Hosted manifest and blob deletion require `unpublish` permission,
which the registry default denies.

## License

Source-available under the [PolyForm Shield License 1.0.0](../../LICENSE.md) —
**not** open source. You may run, modify, and self-host `pnpr` for any purpose
except providing a product that competes with `pnpr` (or with a product the
licensor provides using it). Commercial / non-compete licenses are available
from Zoltan Kochan (<https://kochan.io>).

This is the only part of the pnpm monorepo that is not MIT licensed.

Contributions to `pnpr/` are accepted under separate terms — see
[`CONTRIBUTING.md`](../../CONTRIBUTING.md).

## Trademark notice

pnpr is not affiliated with, endorsed by, or sponsored by npm, Inc., GitHub, or
Microsoft. "npm" is a trademark of npm, Inc., used here only to describe
compatibility with the npm registry protocol.
