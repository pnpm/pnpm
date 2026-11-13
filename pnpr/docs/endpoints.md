---
id: endpoints
title: HTTP endpoints
---

pnpr exposes a set of always-on endpoints (health, user accounts, and tokens)
plus four surfaces:

- **Registry surface** - npm-compatible package, publish, staged-publish, and
  team endpoints, plus the [Cargo, Python, and image](ecosystems.md) protocols.
  Served for the [stored registries](configuration.md#registries-and-defaultregistry);
  `--disable-registry` turns it off for one process. On a resolver-only server
  these routes aren't mounted at all. They answer `404`, not `403`.
- **Resolver surface** (`resolver.enabled`) - pnpr install-accelerator
  endpoints under `/-/pnpr`.
- **Shared-artifact surface** (`artifacts.enabled`) - signed artifact publish,
  lookup, and blob endpoints under `/-/pnpr`, plus the
  [compiler cache](compiler-cache.md). It may run without the resolver.
- **Pipeline run surface** (`pipeline.enabled`) - the
  [`pnpm pipeline` run records](pipeline-runs.md) under `/-/pnpr`.

See [Configuration](configuration.md#registry-resolver-and-artifact-surfaces)
for the config keys and [CLI reference](cli.md) for the command-line overrides.

The npm tables below show paths relative to the npm base, `/npm/`. Each is
equally available under an `/npm/~<name>/` prefix. The always-available
endpoints, the `/-/pnpr/v0/` protocol endpoints, and the image `/v2/` API sit
on the server root. Cargo, Python, and image endpoints are documented on their
own pages.

## npm error responses

Errors on npm registry and account endpoints use `application/json` with matching
`error` and `reason` fields containing the server's explanation. Server faults
hide internal details. A `401` includes a `WWW-Authenticate: Bearer` challenge.
When a supplied token is invalid, the explanation is "Invalid authentication
token" and the challenge includes `error="invalid_token"`. Missing credentials
produce an "Authentication required" explanation.

## Always available

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/ping` | Lightweight reachability check (`npm ping` / `pnpm ping`). Returns `{}`. |
| `GET` | `/-/ready` | Container / Kubernetes readiness probe. Checks that the server is not shutting down and that storage and auth backends are reachable, returning `{"status": "ok", "storage": "ok", "auth": "ok"}` on success or 503 on failure (`{"status": "shutting_down"}` during graceful shutdown). The response also carries `"registries": "ok"`, and the probe fails while the replica [cannot serve the stored registries](configuration.md#replica-reloads). |
| `GET` | `/-/live` | Container / Kubernetes liveness probe. Returns `{"status": "ok"}` when the server process is alive and handling requests. |
| `GET` | `/-/pnpr/v0/sign-in` | The [OIDC providers](oidc.md#browser-sign-in) that offer browser sign-in, as `{"oidc": [{"name": ...}]}`. |
| `GET` | `/-/oidc/{provider}/login` | Starts browser sign-in. With `?return=ui`, sign-in ends in the [web UI](web-ui.md) instead of on a page that shows the token. pnpr refuses `return=ui` when it does not serve the web UI. |
| `POST` | `/-/oidc/handoff?flow=<flow>` | Returns `{"token": ..., "expires": ...}` for the sign-in `flow` that a sign-in with `return=ui` passes to the web UI. The token is released only to the browser that signed in, once, within a minute. |

`GET /-/metrics` and `GET /metrics` serve Prometheus metrics only when [`metrics`](./configuration.md#prometheus-metrics) is configured.

The [user and token endpoints](#user-and-token-endpoints) below are also
always available, whichever surfaces are enabled.

## Registry bases: `/npm/` and `/npm/~<name>/`

Every [registry](configuration.md#registries-and-defaultregistry) is served as
a full npm registry at `/npm/~<name>/`. Packument and tarball reads,
version manifests, dist-tags, search, publish, unpublish, and the login/whoami
endpoints all work under a registry prefix. The `~` prefix keeps registry
names out of the package-name namespace (a package name can never begin with
`~`).

npm publishes require a lowercase, URL-safe package name of at most 214
characters. The package part cannot contain `~'!()*`. Unscoped names cannot
start with `.`, `_`, or `-`, and Node.js built-in module names, `node_modules`,
and `favicon.ico` are reserved. Existing upstream names retain their legacy
read behavior.

The npm base (`/npm/`) aliases the registry named by `defaultRegistry`. With
no `defaultRegistry` stored, it serves no registry and only the
`/npm/~<name>/` bases answer.

Every request routes through the registry graph:

- A request to a **router** is answered by the first listed source whose
  declared [`packages:`](configuration.md#the-packages-map) claim the name. A
  name that no source claims is a definitive `404`. There is no fall-through
  to another origin. A matched source that is unavailable is an error, never a
  `404`.
- A registry's claimed [`packages:`](configuration.md#the-packages-map) patterns
  are enforced **on every path to it**: a read of an unclaimed name is a
  definitive `404` answered before storage or the upstream is consulted (through
  a router, on the default base, and at the registry's own `/~<name>/` URL
  alike), and a publish of an unclaimed name is rejected.
- **Writes** (publish, dist-tags, unpublish) must resolve to a hosted
  registry; a write that routes to an upstream is rejected.
- The serving registry's matching `packages:` rule is enforced on every read
  and write.
- Served `dist.tarball` URLs are rewritten onto `--public-url` (`PNPR_PUBLIC_URL`)
  as `<registry>/<name>@<version>` and stay canonical for the base the client
  addressed (`/npm/` or `/npm/~<name>/`), so lockfiles don't bake in a
  registry name.
- Full package metadata omits `access` when its value is `null`.

## pnpr protocol endpoints

The capability handshake is served when any protocol surface is enabled. It is
also available relative to each npm registry base, at `/npm/-/pnpr` and
`/npm/~<name>/-/pnpr`. At npm bases it advertises only `publish`,
`pnpmPublish`, and `records`. Root-only protocols have empty versions and
features there. Use the server root to discover and call resolver, artifact,
compiler cache, and pipeline endpoints. The `POST` endpoints require a valid pnpr
`Authorization` header.

The response has one capability map, `pnpr.protocols`. Clients must ignore
unknown keys and unknown feature names. A missing protocol or an empty
`versions` array means unsupported. Features apply to the advertised versions;
disabled protocols advertise no features. Clients must select a version they
support before using its features.

| Protocol | Versions when enabled | Features and metadata |
| --- | --- | --- |
| `resolver` | `[0]` | `features`: `fixLockfile`, `verifyLockfile`, `projectTransforms`, `policyEcho`, `errorCodes`, `acceptedLockfileFormats`. `ecosystems` lists accepted package ecosystems. |
| `publish` | `[0]` | Cross-ecosystem publish transaction. |
| `pnpmPublish` | `[1]` | npm batch publish document format. |
| `records` | `[1]` | Enabled when release record signing is configured. `manifestProfiles` lists supported install manifest profiles. |
| `artifacts` | `[0]` | Experimental shared artifacts. |
| `compilerCache` | `[0]` | Experimental compiler cache. |
| `pipeline` | `[0]` | Experimental pipeline run records. |

Every entry includes `versions`, `features`, and `experimental`. The artifacts,
compiler cache, and pipeline protocols set `experimental: true`; their contracts
may change. All pnpr surfaces remain experimental before 1.0.

The supported HTTP protocol path remains `/v0/`, including after pnpr 1.0.
Version numbers identify wire contracts, independently of the server release.
There is no `/v1/` alias or deprecated path. A future incompatible contract must
use a new path and advertise its version alongside the old one during a
documented deprecation window. The `pnpmPublish` and `records` numbers identify
document formats, rather than HTTP path versions.

Project-transform support is advertised by the resolver's `projectTransforms`
feature. Clients check it before sending a request that needs those transforms.
Resolve responses no longer carry a separate capability header.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/pnpr` | Capability handshake returning `pnpr.protocols`. A 404 means every surface is off. |
| `POST` | `/-/pnpr/v0/resolve` | Resolves one project or workspace against the registries and policy sent by the client. Accepts bodies up to 32 MiB and returns `application/x-ndjson` frames. |
| `POST` | `/-/pnpr/v0/verify-lockfile` | Verifies an existing lockfile under the client's registry and policy settings without resolving again. Accepts bodies up to 32 MiB and returns `application/x-ndjson` frames. |

`POST /-/pnpr/v0/resolve` accepts fields such as `projects`, `dependencies`,
`devDependencies`, `optionalDependencies`, `registry`, `registries`,
`overrides`, `lockfile`, `frozenLockfile`, `preferFrozenLockfile`,
`ignoreManifestCheck`, `trustLockfile`, `minimumReleaseAge`,
`minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`,
`trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, and
`fixLockfile`. A repair request regenerates broken
lockfile metadata while retaining compatible locked versions, including for a
filtered install.

The server refuses a request that has a field it does not know. The `400`
response carries the code `ERR_PNPR_UNSUPPORTED_REQUEST_FIELD`, because an
ignored field could be a setting that changes the resolution. The same applies
to fields inside `projects` and to Cargo and Python requests.

`acceptedLockfileFormats` lists the lockfile formats the client reads. The
server refuses a request that lists none it renders with
`ERR_PNPR_INCOMPATIBLE_LOCKFILE_FORMAT`. An absent list means `pnpm-lock/9.0`.

| `lockfileFormat` | Ecosystem | `done.lockfile` |
| --- | --- | --- |
| `pnpm-lock/9.0` | npm | `pnpm-lock.yaml` with `lockfileVersion: '9.0'`, as a JSON object. |
| `cargo-lock` | Cargo | `Cargo.lock`, as a TOML string. |
| `pylock` | Python | `pylock.toml`, as a JSON object. |

pnpm resolves locally when the server refuses a request with either code.

The `registries` map is keyed by URL. Each entry can carry a `prefix` and an
optional `aliases` list. All declared prefixes resolve to that URL. The smallest
prefix in alphabetical order determines package keys in the returned lockfile,
including when the input lockfile uses another declared prefix.

Every registry the request would make pnpr fetch from (the `registry` and
`registries` origins, plus any direct `http(s)`/`git` URL in a dependency
spec, an override, or an input-lockfile tarball) must be on the server's
[fetch allowlist](install-acceleration.md#allowlist). An off-allowlist origin is rejected up front,
before any server-side fetch. Upstream credentials are never taken from the
client: pnpr selects its own server-owned credential per route (see
[Install acceleration](install-acceleration.md#private-registries)), and a URL
carrying inline `user:pass@` credentials is rejected.

The `resolve` and `verify-lockfile` responses are NDJSON streams. Each line is
one JSON object whose `type` names the frame. Clients ignore frame types and
fields they do not know.

| Frame | Terminal | Fields |
| --- | --- | --- |
| `package` | No | `id`, `name`, `version`, `integrity`, `tarball`. Optional: `unpackedSize`, `fileCount`, `revision`, `requiresBuild`. |
| `progress` | No | None. Sent every 15 seconds while a streamed resolve runs, so a proxy with an idle timeout keeps the connection open. |
| `done` | Yes | `resolve`: `lockfileFormat`, `lockfile`, and for npm, `stats.totalPackages` and `appliedPolicies`. `verify-lockfile`: `appliedPolicies`. |
| `violations` | Yes | `violations`: a list of `name`, `version`, `code`, `reason`. The lockfile or an OSV advisory failed the client's policy. |
| `error` | Yes | `message`. Optional: `code` (including `ERR_PNPR_RESOLVE_TIMEOUT` when the request deadline elapses after headers are sent and `ERR_PNPR_GRAPH_TOO_LARGE` when an npm dependency graph exceeds the package limit). |

A stream ends with exactly one terminal frame. Clients reject a `resolve`
`done` frame whose `lockfileFormat` is not one they accepted. For npm `resolve`
and for `verify-lockfile`, clients also reject a `done` frame whose
`appliedPolicies` echo is missing, incomplete, or differs from the requested
policies.

pnpr caps active resolver work at 16 concurrent operations per server instance and 4 per caller identity, queueing additional requests from the same caller behind that caller's own slots. Disconnecting from a streaming `resolve` response aborts the server-side resolution task and releases its slots.

For npm requests, `appliedPolicies` includes `autoInstallPeers`, `dedupePeers`,
`excludeLinksFromLockfile`, `addMissingPeerTypes`, `projectScopedOptionalPeers`,
`frozenLockfile`, `resolutionMode`, `trustLockfile`, `minimumReleaseAge`,
`minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`, `trustPolicy`,
`trustPolicyExclude`, and `trustPolicyIgnoreAfter`. Exclusion lists default to
`[]`. Unset ages and an inactive trust policy are `null`.
The verification endpoint echoes the same shape; clients check its verification
policies without comparing resolution settings.
Upgrade the pnpr server before upgrading clients that require this echo.

### Shared-artifact endpoints

Added in: v0.1.0-alpha.8

These three are mounted when top-level `artifacts.enabled` is true, independently
of `resolver.enabled`. They serve
the [shared side-effects cache](shared-side-effects-cache.md) proof of concept
and, like the other `POST` resolver endpoints, require a pnpr `Authorization`
header. Artifacts are owned by an organization, and
[`artifacts.owners`](configuration.md#registry-resolver-and-artifact-surfaces)
decides who may read and who may publish what each one owns.

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/pnpr/v0/artifacts` | Stores one immutable opaque signed envelope in its organization's namespace. Every blob its manifest names is already stored or carried inline, base64-encoded, up to 64 MiB in all. Requires `publish` on that organization. At most eight variants per input key; a different artifact whose compatible consumers overlap an existing variant gets `409 Conflict`. |
| `PUT` | `/-/pnpr/v0/artifacts/blobs/{organization}/{integrity}` | Stores one blob, streamed from the request body, ahead of the envelope that names it. Requires `publish` on that organization and a `Content-Length` of at most 4 GiB. The blob is stored only if its bytes match the integrity. A blob nothing references yet is kept for at least an hour. |
| `HEAD` | `/-/pnpr/v0/artifacts/blobs/{organization}/{integrity}` | Whether the blob is stored, with its size as `Content-Length`. |
| `GET` | `/-/pnpr/v0/artifacts/blobs/{organization}/{integrity}` | Reads one blob. A blob the caller may not read answers `404`. |
| `POST` | `/-/pnpr/v0/artifacts/resolve` | One batched lookup for candidate input keys. Returns at most eight signed variants per key; scanned envelope bytes plus the serialized response share one 16 MiB budget. |
| `GET` `HEAD` `PUT` | `/-/pnpr/v0/compiler-cache/{cache}/{key}` | One [Cargo compilation cache](compiler-cache.md) entry, over the subset of WebDAV sccache uses. `PROPFIND` answers for virtual parent directories. Added in v0.1.0-alpha.11. |

### Cross-ecosystem endpoints

Added in: v0.1.0-alpha.11

These belong to no single ecosystem, so they sit on the server root.

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/pnpr/v0/publish` | [Publish packages of any ecosystem](ecosystems.md#one-publish-for-a-workspace-that-spans-ecosystems) in one transaction. Advertised as `protocols.publish.versions: [0]` in the handshake. |
| `PUT` | `/-/pnpr/v0/publish/archives/{integrity}` | [Stages one release archive](ecosystems.md#staged-archives) for the caller, streamed from the request body, ahead of the publish that references it. Also under `/npm/`. Answers `201` when stored, `200` when the caller had it staged already, which renews it. Needs a signed-in caller and a `Content-Length` of at most 100 MiB. The archive is stored only if its bytes match the integrity. Only the caller can claim it, for 24 hours after its last upload. |
| `HEAD` | `/-/pnpr/v0/publish/archives/{integrity}` | Whether the caller has the archive staged with at least an hour left, with its size as `Content-Length`. |
| `GET` | `/-/pnpr/v0/registries` | The [registry directory](discovery.md#the-registry-directory): the named registries, ecosystem endpoints, and routing order the caller may see. |

### Publication endpoints {#publication-endpoints}

Served from the [database](storage.md#database), SQLite, libSQL, PostgreSQL, or MySQL.
A publication is a commit that introduced packages. Its id is the namespace
generation it committed. `<namespace>` is `-` for registries in the default namespace.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/publications/<namespace>/<id-or-name>` | The publication, with the members the caller may read: ecosystem, name, version, and artifact (an npm archive's integrity, or another ecosystem's file name). `404` when the caller may read none of them. |
| `GET` | `/-/publications/<namespace>?member=npm:<name>@<version>` | The publication that introduced that npm version. `404` when the caller may not read the package. Members of other ecosystems record no version, so they cannot be looked up this way. |

### Release note endpoints {#release-note-endpoints}

Served from the [database](storage.md#database), SQLite, libSQL, PostgreSQL, or MySQL.
A note is Markdown of at most 64 KiB, with no control characters other than
line feed and tab, no carriage returns, and no bidirectional overrides. A
publish can send notes with it (see
[One publish for a workspace that spans ecosystems](ecosystems.md#one-publish-for-a-workspace-that-spans-ecosystems)),
and a single npm publish document can carry its version note in a top-level
`notes` field.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/npm/<name>/<version>/notes` | The version's note as `text/markdown`. `<version>` may be a dist-tag. `204` when only the version's publication has a note, with a `Link: <…>; rel="publication-notes"` header. A version published with a note carries `PNPR-Note-Initial-Digest`, the digest of the text it was published with, even after an edit or deletion. |
| `PUT`, `DELETE` | `/npm/<name>/<version>/notes` | Replace or delete the version's note. Requires the package's `publish` rule. |
| `GET` | `/npm/<name>/-/notes` | Every note of the package as JSON, newest version first, at most 100 entries and 1 MiB, with `"truncated": true` when more exist. A version whose publication has a note the caller may read names it in `publication`, whose `url` is the note's path on the server, including the path of `public_url`, as in the `Link` header of the version route. |
| `GET`, `PUT`, `DELETE` | `/-/publications/<namespace>/<id-or-name>/notes` | The publication's note. Reading needs read access to every member, writing needs every member's `publish` rule. |

A response that anyone may read is cached `public, max-age=300`. Any other is
`private, no-store`. pnpr keeps every earlier text of a note for recovery.
An unpublished version's note is no longer served or listed. A package that
routes to an upstream registry has no notes: its list is empty, and a single
note answers `404`.

### Pipeline run endpoints

Added in: v0.1.0-alpha.11

Mounted when `pipeline.enabled` is true. See
[Pipeline run records](pipeline-runs.md).

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/pnpr/v0/pipeline/runs` | Record one run. Requires `publish` on its organization. |
| `GET` | `/-/pnpr/v0/pipeline/runs` | Recent run summaries, newest first. Accepts `org`, `workspace`, and `limit`. |
| `GET` | `/-/pnpr/v0/pipeline/runs/{org}/{workspace}/{run_id}` | One run's full record, event stream included. |
| `GET` | `/-/pnpr/v0/pipeline` | A static web viewer over the two read endpoints. |

## Registry read endpoints

These endpoints are mounted when the registry surface is served. Package reads
are checked against the serving registry's matching
[`packages:`](configuration.md#the-packages-map) rule.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/{name}` | Unscoped package packument. |
| `GET` | `/@scope/{name}` | Scoped package packument. |
| `GET` | `/{name}/{version-or-tag}` | Unscoped version manifest by exact version or dist-tag. |
| `GET` | `/@scope/{name}/{version-or-tag}` | Scoped version manifest by exact version or dist-tag. |
| `GET` | `/{name}@{version}/variants/{tag}` | Unscoped [platform variant](#platform-variants) tar. |
| `GET` | `/@scope/{name}@{version}/variants/{tag}` | Scoped platform variant tar. |
| `GET` | `/{name}/{version}+r{revision}` | Unscoped release record. See [pnpr's package document](#pnprs-package-document). |
| `GET` | `/@scope/{name}/{version}+r{revision}` | Scoped release record. |
| `GET` | `/{name}/{version}+r{revision}/evidence` | Unscoped release record's evidence. See [Release record logs](#release-record-logs). |
| `GET` | `/@scope/{name}/{version}+r{revision}/evidence` | Scoped release record's evidence. |
| `GET` | `/-/log/checkpoint` | The latest checkpoint of the registry's release record log. |
| `GET` | `/-/log/tile/{level}/{index}` | A hash tile of the log, `.p/{width}` appended to a partial one. |
| `GET` | `/-/log/tile/entries/{index}` | An entry bundle of the log, only for callers who may read every package it holds. |
| `GET` | `/{name}/-/{filename}` | Unscoped tarball. Not served by a [tar registry](configuration.md#tar-registries). |
| `GET` | `/@scope/{name}/-/{filename}` | Scoped tarball. Not served by a tar registry. |
| `GET` | `/{name}@{version}` | Unscoped tarball by exact version: the `.tgz`, or on a tar registry the tar. |
| `GET` | `/@scope/{name}@{version}` | Scoped tarball by exact version. |
| `GET` | `/{name}@{version}+r{revision}` | Unscoped package tar by exact version and revision. See [Registry revisions](https://pnpm.io/registry-revisions). |
| `GET` | `/@scope/{name}@{version}+r{revision}` | Scoped package tar by exact version and revision. See [Registry revisions](https://pnpm.io/registry-revisions). |
| `GET` | `/-/package/{package}/dist-tags` | Package `dist-tags` object. Use a percent-encoded scoped package name, for example `@scope%2Fname`. |
| `GET` | `/-/v1/search?text=<query>&size=<n>` | npm search v1 shape. Searches locally hosted packages by package-name substring, and the upstreams that opt in with [`search: true`](discovery.md). Results are filtered by access policy. Pass `browse=true` to page through hosted packages with no search term. |
| `GET` | `/-/org/{scope}/package` | Packages of one organization, on registries that enable discovery. Added in v0.1.0-alpha.11. |

If the client sends `Accept: application/vnd.npm.install-v1+json`, pnpr
returns npm's abbreviated install-v1 packument shape. If it sends
`application/vnd.pnpr.install-v2+json`, pnpr returns
[its own package document](#pnprs-package-document). Package documents carry a
`pnpr-registry` response header.

Packument responses emit strong `ETag` and `Vary: Accept` headers at the origin.
A reverse proxy that changes the content coding may weaken the ETag.
[Cloudflare documents this behavior](https://developers.cloudflare.com/cache/reference/etag-headers/).
Public packuments set `Cache-Control: public, max-age=<ttl>` based on upstream or
global packument TTL configuration, while private or caller-scoped responses
set `Cache-Control: private, no-store` and `Vary: Accept, Authorization`.
Conditional requests sending `If-None-Match` or `If-Modified-Since` receive
`304 Not Modified` when unchanged.

When OSV checks are enabled, vulnerable versions are removed from packuments
and dist-tags. Version manifests and tarballs for vulnerable versions are
denied.

The `/{name}@{version}+r{revision}` route answers
[registry revisions](https://pnpm.io/registry-revisions). Each revision's
`dist.integrity` is the SHA-512 of the uncompressed tar, on every registry,
`.tgz` and tar registries alike. The response is the tar with
`Content-Type: application/x-tar`, sent with the `gzip` or `zstd` content coding
the client's `Accept-Encoding` allows, or with none, and `Vary: Accept-Encoding`.
It carries no `ETag`, and a `Range` request gets the whole body.

For a proxied upstream registry, pnpr forwards the request with its own
credentials and accepts a gzip-encoded or plain tar. It decodes the whole
response and checks the tar against the revision's `dist.integrity` in the
upstream packument before it caches or serves any of it. For a hosted registry,
revision `0` serves the tar of the published `.tgz` once its tar integrity has
been verified. Every 3xx fails a revision fetch on the client side, same-origin
included.

pnpr keeps an upstream's revision fields only when they form a complete
revision record: `dist.revision`, including `0` for the original, a
`dist.tarball` on the `<name>@<version>+r<revision>` route, and a
`dist.revisions` history that lists revision `0` once and the selected revision
once. Such a version's `dist` omits `shasum` and `signatures`, which describe a
`.tgz`, on either registry format. npm's documents give the selected revision
by its `+r<revision>` tarball URL and its integrity, without `dist.revision`
and `dist.revisions`. pnpr's own package document marks it with `revision`,
and each revision in the history has its own release record.

When pnpr has verified a version's tarball (at publish time, on first read, or
while streaming and caching an upstream tarball), its full packument,
abbreviated `application/vnd.npm.install-v1+json` packument, and single-version
manifest include `dist.fileCount` and `dist.unpackedSize`. pnpr's own package
document and the version's release record also include `requiresBuild` (`true` when the tarball's root
`package.json` declares `preinstall`, `install`, or `postinstall` under
`scripts`, when the tarball contains a root `binding.gyp` and `package.json`
does not set `gypfile: false`, or when it contains a `.hooks/` entry; `false`
otherwise). When `requiresBuild` is `false` and the package declares
executables via `bin` or `directories.bin`, they also include `bins`, mapping each
command name to its normalized package-relative `path` and optional `prog` /
`args` runtime. Any `dist.pnpr` field supplied by a publisher or an upstream
packument is stripped and never trusted.

### pnpr's package document

A client that sends `Accept: application/vnd.pnpr.install-v2+json` gets pnpr's
own package document, with `Content-Type: application/vnd.pnpr.install-v2+json`:

```json
{
  "name": "fast-thing",
  "distTags": { "latest": "2.1.0" },
  "modified": "2026-10-01T10:00:00.000Z",
  "timesSince": "2026-09-01T10:00:00.000Z",
  "versions": {
    "1.4.0": {},
    "1.4.1": { "deprecated": "1.x is no longer maintained" },
    "1.5.0": { "trust": "provenance", "revision": 2 },
    "2.1.0": {
      "time": "2026-10-01T10:00Z",
      "integrity": "sha512-...",
      "requiresBuild": false,
      "manifest": { "dependencies": { "detect-libc": "^2.0.0" } }
    }
  }
}
```

Each version's entry may have:

- `time`: the publish time, only when the version was published at or after
  `timesSince`, which is [`publishTimeWindow`](configuration.md#packagedocuments)
  before the package's last publish. `null` when pnpr has no publish time.
- `deprecated`, `trust` (`provenance`, `trustedPublisher`, or `stagedPublish`),
  and `revision`, the selected revision of a revision-aware version.
- `integrity`, `fileCount`, `unpackedSize`, `requiresBuild`, `bins`, and
  `manifest`, the publisher's abbreviated manifest without `dist`, `name`,
  `version`, and `deprecated`.

Below [`splitThreshold`](configuration.md#packagedocuments), every entry has
its `manifest`. Above it, only every dist-tag target and the highest release of
each major version (of each minor version below 1.0) keep theirs. Every other
version's manifest and facts are in its release record, at
`/{name}/{version}+r{revision}`:

```json
{
  "name": "fast-thing",
  "version": "1.5.0",
  "integrity": "sha512-...",
  "manifest": { "dependencies": { "detect-libc": "^2.0.0" } }
}
```

A hosted version is accepted with its
release record, and pnpr serves the record's bytes as they were accepted. Such
a record starts with the fields every
[release record](https://github.com/pnpm/rfcs/blob/main/pnpr/text/0000-release-records.md)
has:

```json
{
  "schema": "pnpr-release-record/1",
  "purl": "pkg:npm/fast-thing@1.5.0",
  "name": "fast-thing",
  "version": "1.5.0",
  "revision": 0,
  "interpretation": "npm-install-manifest/1",
  "inputs": {
    "archive": { "kind": "tgz", "integrity": "sha512-..." },
    "packageJson": { "path": "package/package.json", "integrity": "sha512-..." },
    "shrinkwrap": null
  },
  "integrity": "sha512-...",
  "manifest": { "dependencies": { "detect-libc": "^2.0.0" } }
}
```

`interpretation` is the profile the manifest was derived under, and `inputs`
are the digests it was derived from: the `.tgz` as published, and its
`package.json` and `npm-shrinkwrap.json` by their paths in the archive. Each
platform variant in `variants` has `inputs` of its own, and a `purl`: the
record's with a `variant` qualifier naming its tag, with `+` written `%2B`, such
as `pkg:npm/fast-thing@1.5.0?variant=linux-x64%2Blinux-x64-musl`. A registry with
`tarballs: tar` serves the record with the tar's `integrity`.

Revision `0` is the original artifact, also of a version without registry
revisions. A release record never changes once pnpr has verified the archive
it describes, so from then on it is served with
`Cache-Control: public, max-age=31536000, immutable`. Before that, it has no
`Cache-Control` header, since verifying the archive can add `fileCount`,
`unpackedSize`, `requiresBuild`, and `bins`. A package closed to anonymous
callers gets `Cache-Control: private, no-store` for its records.
`/{name}/{version}` keeps serving the selected revision in npm's format.

### Release record logs

With [`releaseRecords.logSigningKey`](configuration.md#releaserecords), each
hosted namespace's release records are appended to a transparency log, in
the [C2SP tiled log](https://c2sp.org/tlog-tiles) format Go's checksum database
uses. A registry address serves the log of the one hosted namespace it
reaches, under `/-/log/`. A registry that reaches no hosted namespace, or
several, serves no log.

- `/-/log/checkpoint` is the log's latest
  [checkpoint](https://c2sp.org/tlog-checkpoint), a signed note. It changes
  as the log grows, so it is served with `Cache-Control: no-cache`.
- `/-/log/tile/{level}/{index}` and `/-/log/tile/entries/{index}` are the
  log's hash tiles and entry bundles. A tile never changes once written, so it
  is cached forever.

Anyone who may read a package of the namespace reads its checkpoint and
hash tiles. An entry bundle names every package logged with it, so only a
caller who may read every package of the namespace gets the entry bundles;
anyone else gets a 404 for every bundle. When some package is closed to
anonymous callers, these responses are `Cache-Control: private, no-store`.

A record's evidence, `/{name}/{version}+r{revision}/evidence`, is read as the
record is:

```json
{
  "record": "sha512-<record digest>",
  "inclusion": { "checkpoint": "<signed note>", "index": 12, "proof": ["<base64 hash>"] },
  "statements": [],
  "sigstore": []
}
```

`record` is the digest of the record's bytes, `index` is its entry's place in
the log, and `proof` is its inclusion proof against the checkpoint. `statements` holds, with
[`releaseRecords.statementSigningKey`](configuration.md#releaserecords), the
registry's acceptance statement: an in-toto Statement in a DSSE envelope, with
predicate type `https://pnpm.io/pnpr/acceptance/v1`, saying the registry
accepted the record under its profile. pnpr signs it from the accepted record
on every read, so a new key signs every statement again. `sigstore` holds the
provenance bundle published with the version, if any. A record whose entry is not in the log yet makes the read wait
for a few seconds; after that it answers `503` with `Retry-After`. The evidence
changes with every newer checkpoint, so it is served with
`Cache-Control: no-cache`.

## Registry write endpoints

These endpoints are mounted when the registry surface is served. `PUT` and
`DELETE` requests require credentials. A read-only bearer token cannot use
write methods, and CIDR-restricted bearer tokens are checked against the peer
socket address. Every write routes through the registry graph and must land on
a hosted registry.

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/{name}` | Publish an unscoped package. Body is the npm publish document with `_attachments`. Requires `publish`. |
| `PUT` | `/@scope/{name}` | Publish a scoped package. Body is the npm publish document with `_attachments`. Requires `publish`. |
| `PUT` | `/-/pnpr/v0/publish` | pnpm batch publish (also mounted on the server root). Body is `{"packages":[<publish document>, ...]}`. The batch is all-or-nothing and each package requires `publish`. See [One publish for a workspace that spans ecosystems](ecosystems.md#one-publish-for-a-workspace-that-spans-ecosystems). |
| `PUT` | `/{package}/-rev/{rev}` | Replace a packument, used by partial unpublish. Requires the current [revision](#packument-revisions) and both `publish` and `unpublish`. Use a percent-encoded scoped package name, for example `@scope%2Fname`. |
| `DELETE` | `/{name}` | Remove an entire unscoped package, used by force unpublish. Requires `unpublish`. Legacy `/{package}/-rev/{rev}` is also accepted. |
| `DELETE` | `/@scope/{name}` | Remove an entire scoped package, used by force unpublish. Requires `unpublish`. Legacy `/@scope/{name}/-rev/{rev}` is also accepted. |
| `DELETE` | `/{name}@{version}` | Unpublish an unscoped version, and reclaim its archives that no other file shares. Unpublishing the last version removes the package. A version the package no longer lists is left as it is. Requires `unpublish`. Legacy `/{name}@{version}/-rev/{rev}` is also accepted. |
| `DELETE` | `/@scope/{name}@{version}` | Unpublish a scoped version, as above. Requires `unpublish`. Legacy `/@scope/{name}@{version}/-rev/{rev}` is also accepted. |
| `DELETE` | `/{name}/-/{filename}` | Unpublish the unscoped version the tarball belongs to, as above. Requires `unpublish`. Legacy `/{name}/-/{filename}/-rev/{rev}` is also accepted. |
| `DELETE` | `/@scope/{name}/-/{filename}` | Unpublish the scoped version the tarball belongs to, as above. Requires `unpublish`. Legacy `/@scope/{name}/-/{filename}/-rev/{rev}` is also accepted. |
| `PUT` | `/-/package/{package}/dist-tags/{tag}` | Add or update a dist-tag. Body is a JSON string naming a published version, for example `"1.0.0"`. Names that parse as semver ranges and targets that are missing or unpublished return `400`. Requires `publish`. |
| `DELETE` | `/-/package/{package}/dist-tags/{tag}` | Remove a dist-tag. Removing `latest` while published versions exist returns `400`. Requires `publish`. |

`dist-tags` in publish requests and package-document updates follows the same
name and target rules. A package-document update cannot remove `latest` while
published versions remain.

Unpublishing a version removes every dist-tag pointing at it. If `latest`
pointed at it and versions remain, `latest` moves to the highest remaining
version. npm visibility and collaborator endpoints return `404` with the
`ERR_PNPR_UNSUPPORTED_ROUTE` code.

Every key under `versions` and its manifest's `version` must be valid SemVer.
pnpr rejects an invalid version with `400`, including versions in DELETE requests.
Publish version keys with build metadata, such as `1.0.0+build.5`, return `400`
with a message explaining that build metadata cannot be published.

### Packument revisions

A hosted package's full metadata document includes `_rev`. A partial unpublish
PUT must use that value in `/{package}/-rev/{rev}`. If the body also includes
`_rev`, it must match the path. A stale revision returns `409` without removing
versions. Read the package metadata again before retrying.

A deprecation PUT to `/{package}` must include the current `_rev`. It changes
only the versions' `deprecated` fields. Dist-tags and other package metadata
stay unchanged. A missing or stale `_rev` returns `409`.

### Platform variants

A hosted version can carry platform variants: archives that pnpm links in
place of the version's own archive on the platforms they declare. Each variant
is a complete package whose `package.json` has the version's `name` and
`version` and declares its platforms as platform tags:

```json
{ "name": "fast-thing", "version": "1.4.0", "variant": { "platforms": ["linux-x64", "linux-x64-musl"] } }
```

A tag is `<os>-<cpu>`, then optionally `musl` or a glibc floor
(`manylinux_2_28`), a macOS or Windows floor (`macos_11_0`, `windows_10_0`), an
N-API floor (`napi8`) or a Node.js or Electron ABI (`node127`, `electron130`);
or `wasm32`. Only the canonical form is accepted.

A variant's tag is its platforms' tags, sorted and joined with `+`, such as
`linux-x64+linux-x64-musl`, and at most 128 bytes long. The publish document
attaches each variant as `<basename>-<version>~<tag>.tgz`, with the `.tgz`'s
`integrity` beside its `data`, or with `integrity` alone for an archive
[staged](ecosystems.md#staged-archives) before the publish, so a version's
archives are not bounded by one request body.
pnpr rejects the version when a variant:

- has another `name` or `version`, or other `peerDependencies` or
  `peerDependenciesMeta`, than the version;
- requires a build;
- has no `variant` field, one that does not parse, or an `os`, `cpu`, or
  `libc` field;
- declares platforms that give another tag than the one that names it;
- lists a platform another variant lists;
- is attached for a version the publish document does not describe, or
  without an `integrity`.

A version published with a provenance statement (its `.sigstore` attachment) is
rejected unless the statement names every archive of the version: the `.tgz`
that `dist.tarball` names by the version's PURL, such as `pkg:npm/%40acme/fast@1.0.0`, and each
variant's `.tgz` by its PURL with the `variant` qualifier, each with its SHA-512.

A variant may differ from the version in its files, dependencies, bundled
dependencies, `bin`, `main`, `exports`, and `engines`. pnpr serves a
variant's tar at `/{name}@{version}/variants/{tag}`, and at
`/{name}@{version}+r{revision}/variants/{tag}` while that revision is selected.

Only [pnpr's package document](#pnprs-package-document) and release records
list the variants, under `variants`: each with its `tag`, structured
`platforms`, tar `integrity`, sizes, `bins`, and a `manifest` of the fields
that differ from the version's (`null` for a removed field). npm's documents
do not list them, so npm clients install the version's own archive, its
`any` archive.

A version can also be published with variants only: its publish document has
no `dist` and no `<basename>-<version>.tgz` attachment. pnpr then generates the
archive npm's documents point to, a `package.json` that lists the union of the
variants' `os`, `cpu`, and Linux `libc`, so npm reports an unsupported platform
instead of installing an empty package. pnpr's package document and the release
record describe such a version without a top-level `integrity`, by the manifest
its variants share: their peer dependencies, and each dependency every variant
declares with the same specifier.

Publishing a version that already exists is a `409`. On the S3 backend, where
several stateless replicas can write to one bucket, concurrent writers are
resolved by conditional writes rather than last-write-wins. A publish or
partial unpublish computed from a packument that another replica has since
changed is rejected with a `409`; a dist-tag write instead replays its mutation
against the fresh packument, and reports `409` only if it keeps losing. Either
way no writer's update is silently discarded. See
[Packument revisions](#packument-revisions) and [Storage backends](storage.md#concurrent-writers).

### Publish-time manifest verification {#publish-time-manifest-verification}

An npm publish document carries each version's metadata beside its archive,
and nothing in the npm protocol requires the two to agree. pnpr verifies every
published `.tgz` against the metadata submitted for it, under the
`npm-install-manifest/1` interpretation profile:

1. The `.tgz` must decode in full to a tar within the decode limit, and the tar
   must name its manifest unambiguously. Each of these is a `400`:
   - an entry of any type whose path escapes the package root;
   - a second entry that an installer would extract to `package.json` or
     `npm-shrinkwrap.json`, including one that differs only in case;
   - a `package.json` or `npm-shrinkwrap.json` that is a link;
   - a missing `package.json`, one larger than 4 MiB, or one that is not a
     JSON object;
   - non-zero data, or more than 10,752 bytes of zeros, after the
     end-of-archive marker;
   - a global PAX header that sets `size`, `path`, or `linkpath`;
   - more than 200,000 entries, or file paths that add up to more than 32 MiB.
2. pnpr derives the install-critical fields from the archive's `package.json`
   and from the submitted version metadata, normalizes both the way npm's
   publish-time `prepare()` and npm 9's `normalize-package-data` rewrite a
   manifest, and compares them. The fields are `name`, `version`,
   `dependencies`, `optionalDependencies`, `peerDependencies`,
   `peerDependenciesMeta`, `bundleDependencies`, `os`, `cpu`, `libc`,
   `engines`, `bin` (with `directories.bin` expanded into it), and the
   `preinstall`, `install`, and `postinstall` scripts, including the
   `node-gyp rebuild` an installer runs for a root `binding.gyp`. The archive's
   `name` and `version` must also name the package and version being
   published. A version compares without semver build metadata, and is
   published without it, as npm publishes it: an archive version
   `1.0.0-canary.1+abc` is published as `1.0.0-canary.1`, and a version with
   build metadata, such as the `+r<N>` of a registry revision, cannot be
   published. Any difference rejects the publish with a `400` whose message
   names the package, the version, and each differing field.
3. One difference is accepted: a submitted `scripts.install: "node-gyp
   rebuild"` that the archive cannot run, because it has no root `binding.gyp`
   or sets `gypfile: false`, while every other lifecycle script agrees. npm adds
   that script whenever any `*.gyp` file is in the source directory, even one
   the archive leaves out. The publish succeeds, the response carries an
   `npm-notice` header that npm prints to the publisher, and the script is not
   served.

The stored version's install-critical fields are then the ones derived from
the archive, and it always carries `hasInstallScript` and `_hasShrinkwrap`.
Fields outside the profile, such as `description` or `repository`, are kept as
submitted. `_archiveManifest` records the profile and the SHA-512 digests of the
archive's exact `package.json` and `npm-shrinkwrap.json` bytes:

```json
"_archiveManifest": {
  "profile": "npm-install-manifest/1",
  "packageJson": "sha512-...",
  "npmShrinkwrap": "sha512-..."
}
```

A staged publish is verified when it is approved.

## Staged publishing endpoints

These implement the server half of [`pnpm stage`](https://pnpm.io/cli/stage), npm's
[staged publishing](https://docs.npmjs.com/staged-publishing) workflow, where a
version is uploaded first and promoted later. A staged version is held back
rather than made visible: it isn't installable, and nothing is written under
the package's name until it is approved. That lets an unattended CI job upload
a release for a human to promote, and it doubles as a review gate: the held
tarball can be downloaded and audited before it goes live.

The two phases check different things. **Staging** validates the publish
document and requires the `publish` right on the package, exactly as a direct
publish does, but does not check whether the version already exists.
**Approval** re-checks the `publish` right against the rules as they stand
then, and is where a version conflict is caught. pnpr raises no
one-time-password challenge of its own at either point. The `--otp` flag on
`pnpm stage approve` is for registries that do.

A stage id is a v4 UUID drawn from the OS CSPRNG, so the id itself acts as an
unguessable handle. Held publish bodies live under a reserved `.staged/` namespace in
the hosted store (both the filesystem and S3 backends). Approval records live
in the database. A record is only visible through the same base it was created with. One staged through
`/npm/~<name>/` is not addressable on the npm base, and vice versa.

| Method | Path | Description |
| --- | --- | --- |
| `POST` | `/-/stage/package/{name}` | Stage a publish. Body is the same npm publish document a `PUT /{name}` publish carries, `_attachments` included. Use a percent-encoded scoped name, for example `@scope%2Fname`. Returns `201` with `{"ok":true,"stageId":"<uuid>"}`. Requires `publish`. |
| `GET` | `/-/stage` | List held records, oldest first. Accepts `page` (zero-based, defaults to `0`), `perPage` (defaults to `100`, clamped to `1` to `100`), and `package` (exact name filter). Returns `{"items":[...],"page":...,"perPage":...,"total":...}`. |
| `GET` | `/-/stage/{id}` | One held record. Requires `publish` on the record's package. |
| `GET` | `/-/stage/{id}/tarball` | The held tarball as `application/octet-stream`, for inspection before approval. Requires `publish`. |
| `POST` | `/-/stage/{id}/approve` | Publish the held document and drop the record. Empty body; returns `201` with `{"ok":true}`. Requires `publish`. |
| `DELETE` | `/-/stage/{id}` | Reject: delete the record and its tarball without publishing. Returns `204`. Requires `publish`. |

Each record carries `id`, `packageName`, `version`, `tag`, `createdAt`,
`actor`, `actorType`, and `shasum`.

Approving a version that landed meanwhile returns `409` and **leaves the record
in place**, so it can still be inspected or rejected.

`GET /-/stage` filters rather than refuses: it always answers `200`, listing
only the records the caller could publish, so an anonymous caller sees an empty
list instead of a `401`. Every other stage endpoint denies loudly (`401` for
anonymous callers, `403` for authenticated ones), matching the publish
endpoint rather than masking as a `404`.

## Team endpoints

pnpr serves npm's team endpoints in the shape [`pnpm team`](https://pnpm.io/cli/team)
consumes. Teams are [registry-scoped](configuration.md#teams).

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/org/{scope}/team` | Teams of the registry that serves `@{scope}`. Requires membership in one of its teams or administrator access. Returns a JSON array of `{"id": ..., "name": ...}`. The scope may be written with or without a leading `@`. |
| `GET` | `/-/team/{scope}/{team}/user` | Members of one team. Requires membership in one of the registry's teams or administrator access. Returns a JSON array of `{"id": ..., "name": ...}`. |
| `PUT` | `/-/org/{scope}/team` | Create a team. The body is `{"name": "<team>"}`. Returns `201`. |
| `DELETE` | `/-/team/{scope}/{team}` | Destroy a team. Returns `200`. |
| `PUT` | `/-/team/{scope}/{team}/user` | Add a member. The body is `{"user": "<username>"}`. Returns `201`. |
| `DELETE` | `/-/team/{scope}/{team}/user` | Remove a member by `{"user": "<username>"}` or `{"userId": "<account-id>"}`. Account IDs also remove deleted members. Returns `204`, also when the user was not a member. A username with no account returns `404`. |

Changes are accepted only on npm hosted registries, and
[change the stored registries](configuration.md#managing-teams-through-the-api):

- The caller must be one of the `auth.admins` or `auth.registryAdmins`.
  Others get `401` when anonymous and `403` when signed in.
- Creating a team that exists, or destroying one a `packages:` rule names,
  returns `409`. Changing a team that does not exist returns `404`.

Reads resolve `@{scope}` to a hosted registry the same way a package read in
that scope would, then check that registry's default `access:` list. Unlike
the stage endpoints, a denial here is **masked as `404`**. Team and member
names must not become an existence probe for a private registry. A registry
that declares no teams returns an empty array.

### Team package access

`npm access grant`, `npm access revoke`, and `npm access list packages
<scope:team>` read and
[edit](configuration.md#changing-rules-through-the-api) the `packages:` rules
of a hosted registry.

The `{team}` path segment is a team name. The stored `team:<id>` token uses
the immutable team ID returned by the team endpoints.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/team/{scope}/{team}/package` | Requires registry team membership or administrator access. The `packages:` keys whose `access` list names `team:<id>` and admits the caller, as `{"<key>": "read-only" \| "read-write"}`. `read-write` means the key's `publish` list names the team too. |
| `PUT` | `/-/team/{scope}/{team}/package` | Grant access. The body is `{"package": "<name>", "permissions": "read-only" \| "read-write"}`. Both add `team:<id>` to the package's `access` list. `read-write` adds it to `publish` too, and `read-only` removes it from `publish`. Returns `201`. |
| `DELETE` | `/-/team/{scope}/{team}/package` | Revoke access. The body is `{"package": "<name>"}`. Removes `team:<id>` from the package's `access`, `publish`, and `unpublish` lists. Returns `204`. |

- The package must be a `packages:` key of its own, such as `'@acme/app'`.
  A package that only a pattern such as `'@acme/*'` covers returns `400`,
  because editing the pattern would grant every package it covers.
- A grant edits the lists that apply to the package now, the same way a
  `PUT` to the [rules admin API](#admin-endpoints) does.
- The caller must be one of the `auth.admins` or `auth.registryAdmins`, and
  the team must exist.

## User and token endpoints

These endpoints are **always mounted** on the server root and under any
`/~<name>/` prefix, whichever surfaces are enabled. A server with an npm
registry also serves them under `/npm/` and `/npm/~<name>/`. `pnpm login`
therefore works against a pnpr npm registry URL, and against the root of a
resolver-only server that exposes no registry surface.

| Method | Path | Description |
| --- | --- | --- |
| `POST` | `/-/v1/login` | Initiates npm/pnpm browser SSO login. Returns `202 Accepted` with `loginUrl` and `doneUrl` when OIDC login is configured, or `404` to fall back to basic auth. |
| `GET` | `/-/v1/login/device` | Browser page for SSO login (`loginUrl`). Asks for the code that `/-/v1/login` sent in its `npm-notice` header. |
| `POST` | `/-/v1/login/device` | Takes the page's form. With one OIDC login provider, or with `provider` set, claims the login with `code` and redirects to the provider. With several providers and no `provider`, checks `code` and returns a page that asks which provider to sign in with. A code works once. pnpr refuses a request without `Sec-Fetch-Site: same-origin`, which current browsers send for a form on a pnpr page. Wrong codes are rate-limited per client address; accepted codes do not count. |
| `GET` | `/-/v1/login/poll/{session}` | CLI polling endpoint (`doneUrl`). Returns `202 Accepted` with `Retry-After: 1` while pending, or `200 OK` with the bearer token once authenticated. |
| `PUT` | `/-/user/{name}` | npm adduser/login endpoint. Also accepts legacy `/-/user/org.couchdb.user:{name}`. Creates a user only when `auth.users.maxUsers` allows registration. Passwords must contain at least 8 characters and at most 71 bytes. Returns a bearer token, which expires after [`auth.tokens.defaultLifetime`](configuration.md#auth), or after `auth.tokens.maxLifetime` when that is shorter. |
| `GET` | `/-/user/{name}` | Requires authentication. Also accepts legacy `/-/user/org.couchdb.user:{name}`. Returns the public user document when the account exists. |
| `GET` | `/-/whoami` | Returns `{"username": ...}` for the authenticated caller. |
| `GET` | `/-/npm/v1/user` | npm profile endpoint. Returns the authenticated username and placeholder profile fields. |
| `POST` | `/-/npm/v1/user` | Change the authenticated user's password. Accepts `{"password":{"old":"current-password","new":"new-password"}}`. Revokes the account's existing bearer tokens. Also revokes browser sessions on the serving replica. |
| `POST` | `/-/npm/v1/tokens` | Creates a bearer token for the authenticated caller. Personal tokens use the `pnpr_` prefix with a base62 payload and CRC-32 checksum. An account with a password must send it as `password`. For an account without one, the caller must authenticate with a browser session or a token, and the new token expires no later than the calling session or token and stays within its CIDR whitelist. Accepts `kind` (`"classic"` or `"granular"`), `readonly` (`true` or `false`) or `packages_and_scopes_permission` (`"read-only"` or `"read-write"`), `cidr_whitelist`, `name`, `expires` (positive integer days or an RFC 3339 timestamp), and `expires_in` (positive integer seconds). Without `expires` or `expires_in`, the token expires after [`auth.tokens.defaultLifetime`](configuration.md#auth). The token expires no later than `auth.tokens.maxLifetime` allows. Unsupported package, scope, organization, and stage-only restrictions return `400`. Expired tokens are rejected at lookup and swept from storage on startup and hourly. |
| `GET` | `/-/npm/v1/tokens` | Lists bearer tokens for the authenticated caller in npm-compatible shape. The server records a token's last use at most once a minute. |
| `DELETE` | `/-/npm/v1/tokens/token/{key}` | Revokes one of the caller's tokens by listing-side token key. |
| `DELETE` | `/-/user/token/{token}` | npm logout endpoint. Revokes the raw bearer token in the path when it belongs to the authenticated caller. |

## Admin endpoints

These endpoints are always mounted, on the server root only. The account
endpoints require one of the [`auth.admins`](configuration.md#auth). The rules
endpoints require one of the `auth.admins` or `auth.registryAdmins`. Others
get `401` when anonymous and `403` when signed in.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/pnpr/v0/admin/users` | Every active account, as `{"users": [{"id": ..., "name": ...}]}`, sorted by name. |
| `POST` | `/-/pnpr/v0/admin/users` | Create an account with `{"name": "...", "password": "..."}`. Returns `201` with `{"id": ..., "name": ...}`, or `409` when the login name is taken. Ignores `maxUsers`. |
| `PUT` | `/-/pnpr/v0/admin/users/{id}` | Replace an account's password with `{"password": "..."}`. Revokes the account's bearer tokens and revokes browser sessions on the serving replica. Returns `200` with `{"id": ..., "name": ...}`, or `404` when the account does not exist. |
| `PATCH` | `/-/pnpr/v0/admin/users/{id}` | Rename an account with `{"name": "..."}`. Its ID, tokens, and grants stay with it. |
| `DELETE` | `/-/pnpr/v0/admin/users/{id}` | Delete the account and revoke its credentials. The login name becomes available; a replacement account receives a fresh ID. Returns `204`. |
| `GET` | `/-/pnpr/v0/admin/users/{id}/tokens` | The account's tokens, in the shape of `GET /-/npm/v1/tokens`. |
| `DELETE` | `/-/pnpr/v0/admin/users/{id}/tokens/{key}` | Revoke one of the account's tokens. Returns `204`, or `404` when the account does not hold that key. |
| `GET` | `/-/pnpr/v0/admin/rules/{ecosystem}/{name}` | The hosted registry's current rules: `access`, and the `access`, `publish`, and `unpublish` lists of each `packages:` key (`null` where the key falls back to the registry's default). `version` (also the `ETag`) names the [revision](#registry-admin-endpoints) of the stored registries. |
| `PUT` | `/-/pnpr/v0/admin/rules/{ecosystem}/{name}` | Replace the lists the body names, and return the rules that result. See [Changing rules through the API](configuration.md#changing-rules-through-the-api). |

To keep two admins from overwriting each other's changes, send the `version`
you read as `If-Match` with a `PUT`. If the stored registries have moved on
since, the request answers `412` and changes nothing. Without `If-Match`, or
with `If-Match: *`, the last change wins. A weak tag such as `W/"..."` never
matches, and an `If-Match` that is not a list of quoted versions answers
`400`. The same rules apply to the
[registry admin endpoints](#registry-admin-endpoints).

A rules body lists only what changes. Each list replaces the current list of
the same place:

```json
{
  "access": ["$authenticated"],
  "packages": {
    "@corp/*": { "access": ["team:<team-id>"], "publish": ["<account-id>"] }
  }
}
```

A key the registry's `packages:` map does not declare, a malformed token, or a
`team:` reference to a team the registry does not have returns `400`. The
`{ecosystem}` and `{name}` are the pair the
[registry directory](discovery.md#the-registry-directory) lists, for example
`npm/private`.

To undo a rules change, [restore](#registry-admin-endpoints) the revision
before it.

### Registry admin endpoints {#registry-admin-endpoints}

These endpoints read and change the
[stored registries](configuration.md#registries-in-the-database). A body or
response is a registries document, with the `registries`, `defaultRegistry`,
and `routes.public` keys, as JSON. A `{name}` is a key of
`registries`.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/pnpr/v0/admin/registries` | The stored registries document. The `ETag` names its version. |
| `PUT` | `/-/pnpr/v0/admin/registries` | Replace the whole document. |
| `GET` | `/-/pnpr/v0/admin/registries/{name}` | One registry's entry. The `ETag` names the version of the whole document. |
| `PUT` | `/-/pnpr/v0/admin/registries/{name}` | Add the registry, or replace its entry. |
| `DELETE` | `/-/pnpr/v0/admin/registries/{name}` | Remove the registry. |
| `GET` | `/-/pnpr/v0/admin/registry-revisions` | Earlier versions, newest first, as `{"revisions": [{"version": ..., "actorId": ..., "createdAt": ..., "restoredFrom": ...}]}`. `?limit=` takes up to 500 and defaults to 50. `?before=<version>` pages back. |
| `GET` | `/-/pnpr/v0/admin/registry-revisions/{version}` | The document of one version. |
| `POST` | `/-/pnpr/v0/admin/registry-revisions/{version}/restore` | Store that version's document as the newest version. |

- The reads require one of the `auth.admins` or `auth.registryAdmins`. The
  changes require one of the `auth.registryAdmins`.
- Every change returns `{"version": ..., "changes": {"added": [...],
  "removed": [...], "changed": [...]}}`. The lists name registries, and
  `changed` also names `defaultRegistry` or `routes` when they change. A
  change that leaves the document as it was stores no new version.
- With `?dryRun=true`, a change is checked and its `changes` returned, but
  nothing is stored.
- Every change accepts `If-Match` as the [rules endpoints](#admin-endpoints)
  do. The version is `0` while nothing is stored.
- A change that pnpr [refuses](configuration.md#changes-pnpr-refuses) stores
  nothing, and the error names the reason.
- Replacing the whole document and restoring a version work while the stored
  document cannot be loaded, so they repair it.

With the default account file store, each replica keeps its own accounts, so an
account change reaches only the replica that served it. Use a
[shared auth backend](auth-backends.md) to run several replicas.

## Error responses

`HEAD` error responses have no body. Other errors from pnpr-owned and npm
endpoints return JSON with `error`, `reason`, and `code` fields.
`error` and `reason` contain the same message.
`code` identifies the error as `ERR_PNPR_<KIND>`, for example
`ERR_PNPR_NOT_FOUND` or `ERR_PNPR_REGISTRIES_UNAVAILABLE`.
HTTP status codes and `Retry-After` headers still describe retry behavior.

Resolution streams report failures in a terminal `type: "error"` frame with
these fields and a `message` field containing the same message.
OCI, Cargo, PyPI, and SCIM protocol endpoints retain their protocol-specific responses.
