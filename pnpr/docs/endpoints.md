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
  these routes aren't mounted at all — they answer `404`, not `403`.
- **Resolver surface** (`resolver.enabled`) - pnpr install-accelerator
  endpoints under `/-/pnpr`.
- **Shared-artifact surface** (`artifacts.enabled`) - signed artifact publish,
  lookup, and blob endpoints under `/-/pnpr`, plus the
  [compiler cache](compiler-cache.md). It may run without the resolver.
- **Pipeline run surface** (`pipeline.enabled`) - the
  [`pnpm pipeline` run records](pipeline-runs.md) under `/-/pnpr`.

See [Configuration](configuration.md#registry-resolver-and-artifact-surfaces)
for the config keys and [CLI reference](cli.md) for the command-line overrides.

The tables below show npm paths on the path-less base. Each is equally available
under a `/~<name>/` prefix and, on a server that declares more than one
ecosystem, under `/npm/`. Cargo, Python, and image endpoints are documented on
their own pages.

## Always available

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/ping` | Health check. Returns `{}`. |

The [user and token endpoints](#user-and-token-endpoints) below are also
always available, whichever surfaces are enabled.

## Registry bases: `/` and `/~<name>/`

Every [registry](configuration.md#registries-and-defaultregistry) is served as
a full npm registry at `/~<name>/` — packument and tarball reads, version
manifests, dist-tags, search, publish, unpublish, and the login/whoami
endpoints all work under a registry prefix. The `~` prefix keeps registry
names out of the package-name namespace (a package name can never begin with
`~`).

The path-less base (`/`) aliases the registry named by `defaultRegistry`; with
no `defaultRegistry` configured, the bare host serves no registry and only the
`/~<name>/` bases answer.

Every request routes through the registry graph:

- A request to a **router** is answered by the first listed source whose
  declared [`packages:`](configuration.md#the-packages-map) claim the name. A
  name that no source claims is a definitive `404` — there is no fall-through
  to another origin. A matched source that is unavailable is an error, never a
  `404`.
- A registry's declared namespace is enforced **on every path to it**: a read
  of a name outside the namespace is a definitive `404` answered before
  storage or the upstream is consulted — through a router, on the path-less
  base, and at the registry's own `/~<name>/` URL alike — and an off-namespace
  publish is rejected.
- **Writes** (publish, dist-tags, unpublish) must resolve to a hosted
  registry; a write that routes to an upstream is rejected.
- The serving registry's matching `packages:` rule is enforced on every read
  and write.
- Served `dist.tarball` URLs are rewritten onto the configured `public_url`
  and stay canonical for the base the client addressed (path-less or
  `/~<name>/`), so lockfiles don't bake in a registry name.

## pnpr protocol endpoints

The capability handshake is mounted when either `resolver.enabled` or
`artifacts.enabled` is true. Each protocol advertises its versions
independently, so resolver-only and artifact-only tiers are both valid. The
`POST` endpoints require a valid pnpr `Authorization` header.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/pnpr` | Capability handshake. Returns `versions` for resolver protocols, `fixLockfile` for resolver versions that support lockfile repair, and `artifacts` for shared-artifact protocols. Each is currently `[0]` when its surface is enabled and `[]` otherwise. A 404 means both protocol surfaces are off. |
| `POST` | `/-/pnpr/v0/resolve` | Resolves one project or workspace against the registries and policy sent by the client. Returns `application/x-ndjson` frames. |
| `POST` | `/-/pnpr/v0/verify-lockfile` | Verifies an existing lockfile under the client's registry and policy settings without resolving again. Returns `application/x-ndjson` frames. |

`POST /-/pnpr/v0/resolve` accepts fields such as `projects`, `dependencies`,
`devDependencies`, `optionalDependencies`, `registry`, `namedRegistries`,
`overrides`, `lockfile`, `frozenLockfile`, `preferFrozenLockfile`,
`ignoreManifestCheck`, `trustLockfile`, `minimumReleaseAge`,
`minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`,
`trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, and
`fixLockfile`. Since v0.1.0-alpha.9, a repair request regenerates broken
lockfile metadata while retaining compatible locked versions, including for a
filtered install.

Every registry the request would make pnpr fetch from — the `registry` and
`namedRegistries` origins, plus any direct `http(s)`/`git` URL in a dependency
spec, an override, or an input-lockfile tarball — must be on the server's fetch
allowlist (the configured registries, declared public routes, the built-in npm
registry, and pnpr's own origin). An off-allowlist origin is rejected up front,
before any server-side fetch. Upstream credentials are never taken from the
client: pnpr selects its own server-owned credential per route (see
[Install acceleration](install-acceleration.md#private-registries)), and a URL
carrying inline `user:pass@` credentials is rejected.

The `resolve` response is an NDJSON stream:

| Frame | Description |
| --- | --- |
| `package` | One resolved tarball: `id`, `name`, `version`, `integrity`, `tarball`, and optional `unpackedSize` / `fileCount`. |
| `done` | Terminal success frame. For `resolve`, carries `lockfile` and `stats.totalPackages`; for `verify-lockfile`, only indicates success. |
| `violations` | Terminal policy failure frame. Carries rendered lockfile or OSV violations. |
| `error` | Terminal server-side resolution or verification error. |

### Shared-artifact endpoints

Added in: v0.1.0-alpha.8

These three are mounted when top-level `artifacts.enabled` is true, independently
of `resolver.enabled`. They serve
the [shared side-effects cache](shared-side-effects-cache.md) proof of concept
and, like the other `POST` resolver endpoints, require a pnpr `Authorization`
header. Artifacts are owner-scoped, and in this proof of concept an
organization's name must equal the authenticated username.

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/pnpr/v0/artifacts` | Stores one immutable opaque signed envelope and its inline content-addressed blobs in the authenticated organization's namespace. At most eight variants per input key; a different artifact whose compatible consumers overlap an existing variant gets `409 Conflict`. |
| `POST` | `/-/pnpr/v0/artifacts/resolve` | One batched lookup for candidate input keys. Returns at most eight signed variants per key; scanned envelope bytes plus the serialized response share one 16 MiB budget. |
| `POST` | `/-/pnpr/v0/artifacts/blob` | Reads one owner-scoped blob by its SHA-512 integrity. |
| `GET` `HEAD` `PUT` | `/-/pnpr/v0/compiler-cache/{cache}/{key}` | One [Cargo compilation cache](compiler-cache.md) entry, over the subset of WebDAV sccache uses. `PROPFIND` answers for virtual parent directories. Added in v0.1.0-alpha.11. |

### Cross-ecosystem endpoints

Added in: v0.1.0-alpha.11

These belong to no single ecosystem, so they stay at the root whatever prefixes
the server mounts.

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
| `PUT` | `/-/pnpr/v0/pipeline/runs` | Record one run. Requires `publish` on its workspace. |
| `GET` | `/-/pnpr/v0/pipeline/runs` | Recent run summaries, newest first. Accepts `workspace` and `limit`. |
| `GET` | `/-/pnpr/v0/pipeline/runs/{workspace}/{run_id}` | One run's full record, event stream included. |
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
| `GET` | `/{name}/-/{filename}` | Unscoped tarball. |
| `GET` | `/@scope/{name}/-/{filename}` | Scoped tarball. |
| `GET` | `/-/package/{package}/dist-tags` | Package `dist-tags` object. Use a percent-encoded scoped package name, for example `@scope%2Fname`. |
| `GET` | `/-/v1/search?text=<query>&size=<n>` | npm search v1 shape. Searches locally hosted packages by package-name substring, and, since v0.1.0-alpha.11, the upstreams that opt in with [`search: true`](discovery.md). Results are filtered by access policy. Pass `browse=true` to page through hosted packages with no search term. |
| `GET` | `/-/org/{scope}/package` | Packages of one organization, on registries that enable discovery. Added in v0.1.0-alpha.11. |
| `GET` | `/-/tarballs/sha512/{digest}` | Immutable artifact by its base64url SHA-512 digest, added in v0.1.0-alpha.8. See [Registry revisions](/registry-revisions). |

If the client sends `Accept: application/vnd.npm.install-v1+json`, pnpr
returns npm's abbreviated install-v1 packument shape.

When OSV checks are enabled, vulnerable versions are removed from packuments
and dist-tags. Version manifests and tarballs for vulnerable versions are
denied.

Since v0.1.0-alpha.8, the digest route answers integrity-addressed
[registry revisions](/registry-revisions). For a proxied upstream registry, pnpr
forwards the request with its own credentials, verifies the integrity, and
caches the object; for a hosted registry, every publish seals a durable
package/version reference under the artifact's SHA-512 digest, so a digest is
resolved with one bounded index read rather than a scan over packuments. Before
the bytes are served, pnpr revalidates the current packument, the revision
history, the caller's access, registry routing, and OSV policy — so a digest URL
is not a way around any of them. Every 3xx fails a digest fetch on the client
side, same-origin included.

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

These implement the server half of [`pnpm stage`](/cli/stage) — npm's
[staged publishing](https://docs.npmjs.com/staged-publishing) workflow, where a
version is uploaded first and promoted later. A staged version is held back
rather than made visible: it isn't installable, and nothing is written under
the package's name until it is approved. That lets an unattended CI job upload
a release for a human to promote, and it doubles as a review gate — the held
tarball can be downloaded and audited before it goes live.

The two phases check different things. **Staging** validates the publish
document and requires the `publish` right on the package, exactly as a direct
publish does, but does not check whether the version already exists.
**Approval** re-checks the `publish` right against the rules as they stand
then, and is where a version conflict is caught. pnpr raises no
one-time-password challenge of its own at either point — the `--otp` flag on
`pnpm stage approve` is for registries that do.

A stage id is a v4 UUID drawn from the OS CSPRNG, so the id itself acts as an
unguessable handle. Held records live under a reserved `.staged/` namespace in
the hosted store (both the filesystem and S3 backends), and a record is only
visible through the same base it was created with — one staged through
`/~<name>/` is not addressable on the path-less base, and vice versa.

| Method | Path | Description |
| --- | --- | --- |
| `POST` | `/-/stage/package/{name}` | Stage a publish. Body is the same npm publish document a `PUT /{name}` publish carries, `_attachments` included. Use a percent-encoded scoped name, for example `@scope%2Fname`. Returns `201` with `{"ok":true,"stageId":"<uuid>"}`. Requires `publish`. |
| `GET` | `/-/stage` | List held records, oldest first. Accepts `page` (zero-based, defaults to `0`), `perPage` (defaults to `100`, clamped to `1`–`100`), and `package` (exact name filter). Returns `{"items":[...],"page":...,"perPage":...,"total":...}`. |
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
list instead of a `401`. Every other stage endpoint denies loudly — `401` for
anonymous callers, `403` for authenticated ones — matching the publish
endpoint rather than masking as a `404`.

## Team endpoints

pnpr serves npm's team-management read endpoints over the teams declared in
its config, in the shape [`pnpm team`](/cli/team) consumes. Teams are
[registry-scoped configuration](configuration.md#teams), so these are
**read-only views**: the listings are served, and every mutation is refused.

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/-/org/{scope}/team` | Teams declared by the registry that serves `@{scope}`. Returns a JSON array of `{"name": ...}`. The scope may be written with or without a leading `@`. |
| `GET` | `/-/team/{scope}/{team}/user` | Members of one team, as a JSON array of `{"name": ...}`. |
| `PUT` | `/-/org/{scope}/team` | Create a team — always `403`. |
| `DELETE` | `/-/team/{scope}/{team}` | Destroy a team — always `403`. |
| `PUT` | `/-/team/{scope}/{team}/user` | Add a member — always `403`. |
| `DELETE` | `/-/team/{scope}/{team}/user` | Remove a member — always `403`. |

A refused mutation carries the error code `teams_config_managed`: teams are
declared in the pnpr configuration, so changing one means updating the config,
not calling the API.

Reads resolve `@{scope}` to a hosted registry the same way a package read in
that scope would, then check that registry's default `access:` list. Unlike
the stage endpoints, a denial here is **masked as `404`** — team and member
names must not become an existence probe for a private registry. A registry
that declares no teams returns an empty array.

## User and token endpoints

These endpoints are **always mounted**, whichever surfaces are enabled, and
they are global — served identically on the path-less base and under any
`/~<name>/` prefix. `pnpm login` therefore works against any pnpr URL,
including a resolver-only server that exposes no registry surface.

| Method | Path | Description |
| --- | --- | --- |
| `PUT` | `/-/user/org.couchdb.user:{name}` | npm adduser/login endpoint. Creates a user only when `auth.htpasswd.max_users` allows registration; existing users can log in with the correct password. Returns a bearer token. |
| `GET` | `/-/whoami` | Returns `{"username": ...}` for the authenticated caller. |
| `GET` | `/-/npm/v1/user` | npm profile endpoint. Returns the authenticated username and placeholder profile fields. |
| `GET` | `/-/npm/v1/tokens` | Lists bearer tokens for the authenticated caller in npm-compatible shape. |
| `DELETE` | `/-/npm/v1/tokens/token/{key}` | Revokes one of the caller's tokens by listing-side token key. |
| `DELETE` | `/-/user/token/{token}` | npm logout endpoint. Revokes the raw bearer token in the path when it belongs to the authenticated caller. |
