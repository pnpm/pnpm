---
id: endpoints
title: HTTP endpoints
---

pnpr exposes a set of always-on endpoints (health, user accounts, and tokens)
plus four surfaces:

- **Registry surface** - npm-compatible package, publish, staged-publish, and
  team endpoints, plus the [Cargo, Python, and image](ecosystems.md) protocols.
  Served exactly when at least one registry is declared under
  [`registries:`](configuration.md#registries-and-defaultregistry);
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

## Always available

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/ping` | Health check. Returns `{}` on success, or 503 if storage or auth is unavailable. |
| `GET` | `/-/ready` | Container / Kubernetes readiness probe. Checks storage and auth backends, returning `{"status": "ok", "storage": "ok", "auth": "ok"}` on success or 503 on failure. |
| `GET` | `/-/live` | Container / Kubernetes liveness probe. Returns `{"status": "ok"}` when the server process is alive and handling requests. |
| `GET` | `/-/metrics` | Unauthenticated Prometheus metrics endpoint. Emits standard Prometheus text format (`text/plain; version=0.0.4`) exposing request rates, durations, response bytes, in-flight gauges, and cache stats. Unauthenticated access is intentional for cluster monitoring and metrics scrapers. |
| `GET` | `/metrics` | Unauthenticated Prometheus metrics endpoint for scrapers. |
| `GET` | `/-/pnpr/v0/sign-in` | The [OIDC providers](oidc.md#browser-sign-in) that offer browser sign-in, as `{"oidc": [{"name": ...}]}`. |
| `GET` | `/-/oidc/{provider}/login` | Starts browser sign-in. With `?return=ui`, sign-in ends in the [web UI](web-ui.md) instead of on a page that shows the token. pnpr refuses `return=ui` when it does not serve the web UI. |
| `POST` | `/-/oidc/handoff?flow=<flow>` | Returns `{"token": ..., "expires": ...}` for the sign-in `flow` that a sign-in with `return=ui` passes to the web UI. The token is released only to the browser that signed in, once, within a minute. |

The [user and token endpoints](#user-and-token-endpoints) below are also
always available, whichever surfaces are enabled.

## Registry bases: `/npm/` and `/npm/~<name>/`

Every [registry](configuration.md#registries-and-defaultregistry) is served as
a full npm registry at `/npm/~<name>/`. Packument and tarball reads,
version manifests, dist-tags, search, publish, unpublish, and the login/whoami
endpoints all work under a registry prefix. The `~` prefix keeps registry
names out of the package-name namespace (a package name can never begin with
`~`).

The npm base (`/npm/`) aliases the registry named by `defaultRegistry`. With
no `defaultRegistry` configured, it serves no registry and only the
`/npm/~<name>/` bases answer.

Every request routes through the registry graph:

- A request to a **router** is answered by the first listed source whose
  declared [`packages:`](configuration.md#the-packages-map) claim the name. A
  name that no source claims is a definitive `404`. There is no fall-through
  to another origin. A matched source that is unavailable is an error, never a
  `404`.
- A registry's declared namespace is enforced **on every path to it**: a read
  of a name outside the namespace is a definitive `404` answered before
  storage or the upstream is consulted (through a router, on the default
  base, and at the registry's own `/~<name>/` URL alike), and an off-namespace
  publish is rejected.
- **Writes** (publish, dist-tags, unpublish) must resolve to a hosted
  registry; a write that routes to an upstream is rejected.
- The serving registry's matching `packages:` rule is enforced on every read
  and write.
- Served `dist.tarball` URLs are rewritten onto `--public-url` (`PNPR_PUBLIC_URL`)
  and stay canonical for the base the client addressed (`/npm/` or
  `/npm/~<name>/`), so lockfiles don't bake in a registry name.

## pnpr protocol endpoints

The capability handshake is served when any of the four surfaces is enabled.
Each protocol advertises its versions independently, so resolver-only and
artifact-only tiers are both valid. The `POST` endpoints require a valid pnpr
`Authorization` header.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/pnpr` | Capability handshake. Returns `versions` for resolver protocols, `fixLockfile` for resolver versions that support lockfile repair, `ecosystems` for the ecosystems the resolver serves, `artifacts` for shared-artifact protocols, `publish` for the cross-ecosystem publish transaction, and `pipeline` for pipeline run records. Each version list is `[0]` when its surface is enabled and `[]` otherwise. A 404 means every surface is off. |
| `POST` | `/-/pnpr/v0/resolve` | Resolves one project or workspace against the registries and policy sent by the client. Returns `application/x-ndjson` frames. |
| `POST` | `/-/pnpr/v0/verify-lockfile` | Verifies an existing lockfile under the client's registry and policy settings without resolving again. Returns `application/x-ndjson` frames. |

`POST /-/pnpr/v0/resolve` accepts fields such as `projects`, `dependencies`,
`devDependencies`, `optionalDependencies`, `registry`, `registries`,
`overrides`, `lockfile`, `frozenLockfile`, `preferFrozenLockfile`,
`ignoreManifestCheck`, `trustLockfile`, `minimumReleaseAge`,
`minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`,
`trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, and
`fixLockfile`. A repair request regenerates broken
lockfile metadata while retaining compatible locked versions, including for a
filtered install.

Every registry the request would make pnpr fetch from (the `registry` and
`registries` origins, plus any direct `http(s)`/`git` URL in a dependency
spec, an override, or an input-lockfile tarball) must be on the server's
[fetch allowlist](install-acceleration.md#allowlist). An off-allowlist origin is rejected up front,
before any server-side fetch. Upstream credentials are never taken from the
client: pnpr selects its own server-owned credential per route (see
[Install acceleration](install-acceleration.md#private-registries)), and a URL
carrying inline `user:pass@` credentials is rejected.

The `resolve` response is an NDJSON stream:

| Frame | Description |
| --- | --- |
| `package` | One resolved tarball: `id`, `name`, `version`, `integrity`, `tarball`, and optional `unpackedSize` / `fileCount` / `requiresBuild`. |
| `done` | Terminal success frame. For `resolve`, carries `lockfile` and `stats.totalPackages`; for `verify-lockfile`, only indicates success. |
| `violations` | Terminal policy failure frame. Carries rendered lockfile or OSV violations. |
| `error` | Terminal server-side resolution or verification error. |

### Shared-artifact endpoints

Added in: v0.1.0-alpha.8

These three are mounted when top-level `artifacts.enabled` is true, independently
of `resolver.enabled`. They serve
the [shared side-effects cache](shared-side-effects-cache.md) proof of concept
and, like the other `POST` resolver endpoints, require a pnpr `Authorization`
header. Artifacts are owned by an organization, and
[`artifacts.orgs`](configuration.md#registry-resolver-and-artifact-surfaces)
decides who may read and who may publish what each one owns.

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/pnpr/v0/artifacts` | Stores one immutable opaque signed envelope in its organization's namespace. Every blob its manifest names is already stored or carried inline, base64-encoded, up to 64 MiB in all. Requires `publish` on that organization. At most eight variants per input key; a different artifact whose compatible consumers overlap an existing variant gets `409 Conflict`. |
| `PUT` | `/-/pnpr/v0/artifacts/blob?organization=<name>&integrity=<sha512-…>` | Stores one blob, streamed from the request body, ahead of the envelope that names it. Requires `publish` on that organization and a `Content-Length` of at most 4 GiB. The blob is stored only if its bytes match the integrity. A blob nothing references yet is kept for at least an hour. |
| `HEAD` | `/-/pnpr/v0/artifacts/blob?organization=<name>&integrity=<sha512-…>` | Whether the blob is stored, with its size as `Content-Length`. |
| `POST` | `/-/pnpr/v0/artifacts/resolve` | One batched lookup for candidate input keys. Returns at most eight signed variants per key; scanned envelope bytes plus the serialized response share one 16 MiB budget. |
| `POST` | `/-/pnpr/v0/artifacts/blob` | Reads one owner-scoped blob by its SHA-512 integrity. |
| `GET` `HEAD` `PUT` | `/-/pnpr/v0/compiler-cache/{cache}/{key}` | One [Cargo compilation cache](compiler-cache.md) entry, over the subset of WebDAV sccache uses. `PROPFIND` answers for virtual parent directories. Added in v0.1.0-alpha.11. |

### Cross-ecosystem endpoints

Added in: v0.1.0-alpha.11

These belong to no single ecosystem, so they sit on the server root.

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/pnpr/v0/publish` | [Publish packages of any ecosystem](ecosystems.md#one-publish-for-a-workspace-that-spans-ecosystems) in one journaled transaction. Advertised as `publish: [0]` in the handshake. |
| `GET` | `/-/pnpr/v0/registries` | The [registry directory](discovery.md#the-registry-directory): the named registries, ecosystem endpoints, and routing order the caller may see. |

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
| `GET` | `/{name}/-/{filename}` | Unscoped tarball. Not served by a [tar registry](configuration.md#tar-registries). |
| `GET` | `/@scope/{name}/-/{filename}` | Scoped tarball. Not served by a tar registry. |
| `GET` | `/{name}@{version}` | Unscoped tarball by exact version: the `.tgz`, or on a tar registry the tar. |
| `GET` | `/@scope/{name}@{version}` | Scoped tarball by exact version. |
| `GET` | `/-/package/{package}/dist-tags` | Package `dist-tags` object. Use a percent-encoded scoped package name, for example `@scope%2Fname`. |
| `GET` | `/-/v1/search?text=<query>&size=<n>` | npm search v1 shape. Searches locally hosted packages by package-name substring, and the upstreams that opt in with [`search: true`](discovery.md). Results are filtered by access policy. Pass `browse=true` to page through hosted packages with no search term. |
| `GET` | `/-/org/{scope}/package` | Packages of one organization, on registries that enable discovery. Added in v0.1.0-alpha.11. |
| `GET` | `/-/tarballs/sha512/{digest}` | Immutable package tar by the base64url SHA-512 digest of the uncompressed tar, added in v0.1.0-alpha.8. See [Registry revisions](https://pnpm.io/registry-revisions). |

If the client sends `Accept: application/vnd.npm.install-v1+json`, pnpr
returns npm's abbreviated install-v1 packument shape.

Packument responses emit strong `ETag` and `Vary: Accept` headers. Public
packuments set `Cache-Control: public, max-age=<ttl>` based on upstream or
global packument TTL configuration, while private or caller-scoped responses
set `Cache-Control: private, no-store` and `Vary: Accept, Authorization`.
Conditional requests sending `If-None-Match` or `If-Modified-Since` receive
`304 Not Modified` when unchanged.

When OSV checks are enabled, vulnerable versions are removed from packuments
and dist-tags. Version manifests and tarballs for vulnerable versions are
denied.

The digest route answers integrity-addressed
[registry revisions](https://pnpm.io/registry-revisions). The digest is the SHA-512 of the
uncompressed tar, on every registry, `.tgz` and tar registries alike. The
response is the tar with `Content-Type: application/x-tar`, sent with the
`gzip` or `zstd` content coding the client's `Accept-Encoding` allows, or with
none, and `Vary: Accept-Encoding`. It carries no `ETag`, and a `Range` request
gets the whole body.

For a proxied upstream registry, pnpr forwards the request with its own
credentials and accepts a gzip-encoded or plain tar. It decodes the whole
response and checks the tar against the digest before it caches or serves any
of it. For a hosted registry, every publish decodes the `.tgz` and seals a
durable package/version reference under the digest of its tar, so a digest is
resolved with one bounded index read rather than a scan over packuments. A
`.tgz` whose gzip stream does not decode in full has no tar, and gets no
reference. Before the bytes are served, pnpr revalidates the current packument,
the caller's access, registry routing, and OSV policy, so a digest URL is not a
way around any of them. Every 3xx fails a digest fetch on the client side,
same-origin included.

Package metadata keeps an upstream's revision fields only when they form a
complete revision record: `dist.revision`, including `0` for the original, a
`dist.tarball` on the digest route, and a `dist.revisions` history that lists
revision `0` once and the selected revision once. Such a version's `dist`
omits `shasum` and `signatures`, which describe a `.tgz`, on either registry
format. Any other version is served as ordinary metadata, without `revision`
or `revisions`.

When pnpr has verified a version's tarball (at publish time, on first read, or
while streaming and caching an upstream tarball), its full packument,
abbreviated `application/vnd.npm.install-v1+json` packument, and single-version
manifest include `dist.fileCount`, `dist.unpackedSize`,
`dist.pnpr.requiresBuild` (`true` when the tarball's root
`package.json` declares `preinstall`, `install`, or `postinstall` under
`scripts`, when the tarball contains a root `binding.gyp` and `package.json`
does not set `gypfile: false`, or when it contains a `.hooks/` entry; `false`
otherwise). When `requiresBuild` is `false` and the package declares
executables via `bin` or `directories.bin`, they also include `dist.pnpr.bins`, mapping each
command name to its normalized package-relative `path` and optional `prog` /
`args` runtime. Any `dist.pnpr` field supplied by a publisher or an upstream
packument is stripped and never trusted.

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
| `PUT` | `/-/pnpm/v1/publish` | pnpm batch publish. Body is `{"packages":[<publish document>, ...]}`. The batch is all-or-nothing and each package requires `publish`. For a release that spans ecosystems, use [`PUT /-/pnpr/v0/publish`](ecosystems.md#one-publish-for-a-workspace-that-spans-ecosystems) instead. |
| `PUT` | `/{package}/-rev/{rev}` | Replace a packument, used by partial unpublish. Requires both `publish` and `unpublish`. Use a percent-encoded scoped package name, for example `@scope%2Fname`. |
| `DELETE` | `/{package}/-rev/{rev}` | Remove an entire package, used by force unpublish. Requires `unpublish`. Use a percent-encoded scoped package name for scoped packages. |
| `DELETE` | `/{name}/-/{filename}/-rev/{rev}` | Remove an unscoped tarball. Requires `unpublish`. |
| `DELETE` | `/@scope/{name}/-/{filename}/-rev/{rev}` | Remove a scoped tarball. Requires `unpublish`. |
| `PUT` | `/-/package/{package}/dist-tags/{tag}` | Add or update a dist-tag. Body is a JSON string version, for example `"1.0.0"`. Requires `publish`. |
| `DELETE` | `/-/package/{package}/dist-tags/{tag}` | Remove a dist-tag. Requires `publish`. |

Publishing a version that already exists is a `409`. On the S3 backend, where
several stateless replicas can write to one bucket, concurrent writers are
resolved by conditional writes rather than last-write-wins. A publish or
partial unpublish computed from a packument that another replica has since
changed is rejected with a `409`; a dist-tag write instead replays its mutation
against the fresh packument, and reports `409` only if it keeps losing. Either
way no writer's update is silently discarded. See
[Storage backends](storage.md#concurrent-writers).

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
unguessable handle. Held records live under a reserved `.staged/` namespace in
the hosted store (both the filesystem and S3 backends), and a record is only
visible through the same base it was created with. One staged through
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
| `GET` | `/-/org/{scope}/team` | Teams of the registry that serves `@{scope}`. Returns a JSON array of `{"name": ...}`. The scope may be written with or without a leading `@`. |
| `GET` | `/-/team/{scope}/{team}/user` | Members of one team, as a JSON array of `{"name": ...}`. |
| `PUT` | `/-/org/{scope}/team` | Create a team. The body is `{"name": "<team>"}`. Returns `201`. |
| `DELETE` | `/-/team/{scope}/{team}` | Destroy a team. Returns `200`. |
| `PUT` | `/-/team/{scope}/{team}/user` | Add a member. The body is `{"user": "<username>"}`. Returns `201`. |
| `DELETE` | `/-/team/{scope}/{team}/user` | Remove a member. The body is `{"user": "<username>"}`. Returns `204`, also when the user was not a member. |

Changes are accepted only on a registry with
[`teamsManagedBy: api`](configuration.md#managing-teams-through-the-api):

- On any other registry a change returns `403` with the error code
  `teams_config_managed`.
- The caller must be one of the `auth.admins`. Others get `401` when
  anonymous and `403` when signed in.
- Creating a team that exists, or destroying one a `packages:` rule names,
  returns `409`. Changing a team that does not exist returns `404`.

Reads resolve `@{scope}` to a hosted registry the same way a package read in
that scope would, then check that registry's default `access:` list. Unlike
the stage endpoints, a denial here is **masked as `404`**. Team and member
names must not become an existence probe for a private registry. A registry
that declares no teams returns an empty array.

### Team package access

`npm access grant`, `npm access revoke`, and `npm access list packages
<scope:team>` edit and read the `packages:` rules of a registry with
[`rulesManagedBy: api`](configuration.md#changing-rules-through-the-api).

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/team/{scope}/{team}/package` | The `packages:` keys whose `access` list names `team:<team>` and admits the caller, as `{"<key>": "read-only" \| "read-write"}`. `read-write` means the key's `publish` list names the team too. |
| `PUT` | `/-/team/{scope}/{team}/package` | Grant access. The body is `{"package": "<name>", "permissions": "read-only" \| "read-write"}`. Both add `team:<team>` to the package's `access` list. `read-write` adds it to `publish` too, and `read-only` removes it from `publish`. Returns `201`. |
| `DELETE` | `/-/team/{scope}/{team}/package` | Revoke access. The body is `{"package": "<name>"}`. Removes `team:<team>` from the package's `access`, `publish`, and `unpublish` lists. Returns `204`. |

- The package must be a `packages:` key of its own, such as `'@acme/app'`.
  A package that only a pattern such as `'@acme/*'` covers returns `400`,
  because editing the pattern would grant every package it covers.
- A grant edits the lists that apply to the package now, and stores only the
  lists it changes, the same way a `PUT` to the
  [rules admin API](#admin-endpoints) does.
- The caller must be one of the `auth.admins`, and the team must exist.
- On a registry without `rulesManagedBy: api` a change returns `403` with
  `rules_config_managed`.

## User and token endpoints

These endpoints are **always mounted** on the server root and under any
`/~<name>/` prefix, whichever surfaces are enabled. A server with an npm
registry also serves them under `/npm/` and `/npm/~<name>/`. `pnpm login`
therefore works against a pnpr npm registry URL, and against the root of a
resolver-only server that exposes no registry surface.

| Method | Path | Description |
| --- | --- | --- |
| `POST` | `/-/v1/login` | Initiates npm/pnpm browser SSO login. Returns `202 Accepted` with `loginUrl` and `doneUrl` when OIDC login is configured, or `404` to fall back to basic auth. |
| `GET` | `/-/v1/login/start` | Browser entry point for SSO login. Validates the session and redirects to the configured OIDC provider or renders a provider picker. |
| `GET` | `/-/v1/login/poll/{session}` | CLI polling endpoint (`doneUrl`). Returns `202 Accepted` with `Retry-After: 1` while pending, or `200 OK` with the bearer token once authenticated. |
| `PUT` | `/-/user/org.couchdb.user:{name}` | npm adduser/login endpoint. Creates a user only when `auth.htpasswd.max_users` allows registration; existing users can log in with the correct password. Returns a bearer token. |
| `GET` | `/-/whoami` | Returns `{"username": ...}` for the authenticated caller. |
| `GET` | `/-/npm/v1/user` | npm profile endpoint. Returns the authenticated username and placeholder profile fields. |
| `GET` | `/-/npm/v1/tokens` | Lists bearer tokens for the authenticated caller in npm-compatible shape. The server records a token's last use at most once a minute. |
| `DELETE` | `/-/npm/v1/tokens/token/{key}` | Revokes one of the caller's tokens by listing-side token key. |
| `DELETE` | `/-/user/token/{token}` | npm logout endpoint. Revokes the raw bearer token in the path when it belongs to the authenticated caller. |

## Admin endpoints

These endpoints are always mounted, on the server root only. Each requires
one of the [`auth.admins`](configuration.md#auth). Others get `401` when
anonymous and `403` when signed in.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/pnpr/v0/admin/users` | Every account, as `{"users": [{"name": ...}]}`, sorted by name. |
| `PUT` | `/-/pnpr/v0/admin/users/{name}` | Create the account (`201`) or replace its password (`200`). The body is `{"password": "..."}`. Creating ignores `max_users`. |
| `DELETE` | `/-/pnpr/v0/admin/users/{name}` | Remove the account and revoke every token it holds. Returns `204`. The account's tokens stop working as soon as the account is gone, even before they are revoked. If the request fails, repeat it to revoke what is left. |
| `GET` | `/-/pnpr/v0/admin/users/{name}/tokens` | The account's tokens, in the shape of `GET /-/npm/v1/tokens`. |
| `DELETE` | `/-/pnpr/v0/admin/users/{name}/tokens/{key}` | Revoke one of the account's tokens. Returns `204`, or `404` when the account does not hold that key. |
| `GET` | `/-/pnpr/v0/admin/rules/{ecosystem}/{name}` | The hosted registry's current rules: `access`, and the `access`, `publish`, and `unpublish` lists of each `packages:` key (`null` where the key falls back to the registry's default). `rulesManagedBy` says whether they can be changed, and `version` (also the `ETag`) names the stored changes. |
| `PUT` | `/-/pnpr/v0/admin/rules/{ecosystem}/{name}` | Replace the stored rule changes with the body, and return the rules that result. Only on a registry with [`rulesManagedBy: api`](configuration.md#changing-rules-through-the-api); elsewhere `403` with `rules_config_managed`. |
| `DELETE` | `/-/pnpr/v0/admin/rules/{ecosystem}/{name}` | Drop the stored rule changes, so the config's rules apply again. Returns `204`. |

To keep two admins from overwriting each other's changes, send the `version`
you read as `If-Match` with a `PUT` or `DELETE`. If the stored changes have
moved on since, the request answers `412` and changes nothing. Without
`If-Match`, or with `If-Match: *`, the last change wins. A weak tag such as
`W/"..."` never matches, and an `If-Match` that is not a list of quoted
versions answers `400`.

A rules body lists only what changes. Each list replaces the config's list of
the same place:

```json
{
  "access": ["$authenticated"],
  "packages": {
    "@corp/*": { "access": ["team:platform"], "publish": ["alice"] }
  }
}
```

A key the registry's `packages:` map does not declare, a malformed token, or a
`team:` reference to a team the registry does not have returns `400`. The
`{ecosystem}` and `{name}` are the pair the
[registry directory](discovery.md#the-registry-directory) lists, for example
`npm/private`.

With the default htpasswd store, each replica keeps its own accounts, so an
account change reaches only the replica that served it. Use a
[shared auth backend](auth-backends.md) to run several replicas.
