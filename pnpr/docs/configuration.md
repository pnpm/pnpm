---
id: configuration
title: Configuration
---

pnpr uses a YAML config. When you don't pass `-c`, pnpr looks for a global
`config.yaml` in its config directory and otherwise falls back to a bundled
default (which is shaped for pnpm's own test registry — write your own config
for real use).

A minimal config that hosts a private scope locally and routes everything else
to the public npm registry:

```yaml title="pnpr.yaml"
storage: ./storage

registries:
  local:
    type: hosted
    packages:
      # The names this registry serves, plus their access rules.
      '@mycompany/*':
        access: $authenticated
        publish: $authenticated

  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true

  main:
    type: router
    sources: [local, npmjs]

defaultRegistry: main
```

Pass it with `-c`:

```sh
pnpr -c ./pnpr.yaml
```

## `storage` and `cache`

pnpr keeps two kinds of data:

- **`storage`** — the source of truth: packages published to this server plus
  anything served in static mode. Back this up and keep it on a durable volume.
- **`cache`** — the disposable mirror of upstream registries plus the resolver
  store, resolver cache, lockfile-verdict cache, local shared-artifact store,
  and S3 upload staging scratch.
  Safe to wipe at any time. Defaults to a `.pnpr-cache` subdirectory of
  `storage`; point it at a separate, ephemeral volume to keep cached upstream
  content off the durable disk.

```yaml title="pnpr.yaml"
storage: ./storage

# Path string:
#cache: ./cache

# Or settings table with budget and retention policies:
#cache:
#  path: ./cache     # defaults to .pnpr-cache in storage
#  maxSize: 20GB     # enforces LRU eviction when total size exceeds this
#  maxAge: 30d       # TTL deletion of cached entries older than this
```

When `maxSize` or `maxAge` is configured, pnpr evicts stale entries on startup
and runs periodic eviction in the background. On startup, pnpr also prunes
orphaned private upstream namespaces left behind by previous process runs using
ephemeral per-process secrets. Use [`pnpr cache-gc`](cli.md#cache-gc) for
on-demand or offline reclamation.

The hosted store can instead live in an S3-compatible object store — see
[Storage backends](storage.md).

## `registries` and `defaultRegistry`

The `registries:` map is pnpr's **only routing surface**. Every addressable
registry origin is a named registry, exposed at `https://<pnpr>/~<name>/`. There
are three kinds:

- **`hosted`** — a registry pnpr itself is the authoritative origin for. The
  only kind that accepts writes (publish, dist-tags, unpublish).
- **`upstream`** — exactly one external registry origin, proxied and cached.
- **`router`** — an ordered list of concrete source registries; a package
  resolves to the first listed source that claims its name.

The model is governed by one invariant: **provenance is declared, never
inferred**. Every concrete registry declares the package names it serves — its
namespace — through its [`packages:` map](#the-packages-map), a package
resolves to exactly one declared origin, and no configuration can express a
cross-origin fall-through — not on "not found" and not on "unavailable". The
namespace is enforced at the registry, on every path to it: a read of a name
outside the namespace is a definitive 404 answered before storage or the
upstream is consulted, and an off-namespace publish is rejected. This closes
the dependency-confusion class of attacks by construction.

`defaultRegistry` names the registry that the path-less base URL
(`https://<pnpr>/`) aliases — usually a router. When omitted, the bare host
serves no registry and clients must address a `/~<name>/` URL.

A registry name is served as the single URL path segment `/~<name>/`, so it
must be one URL-safe segment: it cannot be empty, `.` or `..`, start with `.`,
or contain `/`, `\`, `:`, `%`, `?`, `#`, whitespace, or control characters. The
`~` must arrive unencoded: `%7E` does not select the named registry.

Since v0.1.0-alpha.11, a registry also declares which package ecosystem it
serves, through `ecosystem:` (`npm`, `cargo`, `pypi`, or `oci`; npm by default).
A server that serves more than one addresses them through `/npm/`, `/cargo/`,
`/pypi/`, and `/oci/` path prefixes, and registry names can be reused across ecosystems
by grouping the entries under an ecosystem key. See
[Cargo and Python registries](ecosystems.md) and
[Container images](container-images.md).

### The `packages` map

Every concrete registry (hosted or upstream) takes an optional `packages:`
map. Its **keys are the registry's namespace** — the package names it serves —
and its **values are the per-package access rules**. One declaration routes,
filters, and authorizes. When the map is omitted, the registry serves every
name.

Keys use a deliberately small pattern language, so coverage can be checked
statically:

| Pattern | Matches |
| --- | --- |
| `**` | Every package name. |
| `@*/*` | Every scoped package, any scope. |
| `@scope/*` | Every package in one scope. |
| `foo`, `@scope/foo` | Exactly that package. |

Any other wildcard is a config error rather than a silently-never-matching
literal.

The **most specific matching key wins**: an exact name beats `@scope/*`, which
beats `@*/*`, which beats `**`. Key order carries no meaning — at most one key
per specificity tier can match a name, so the winning rule is unique and
reordering the map (by a formatter, or a YAML round-trip) can never change
which rule applies. A duplicate key is a config error.

Each value can set `access`, `publish`, and `unpublish`. When a field is
omitted, `access` falls back to the registry-level `access:` default, `publish`
defaults to `$authenticated`, and `unpublish` defaults to nobody.

Every one of those fields takes either a single access token or a YAML list of
them, and the grammar is deliberately small:

| Token | Admits |
| --- | --- |
| `$all` | Anyone, authenticated or not. |
| `$authenticated` | Any logged-in user. |
| `$anonymous` | Callers with no credentials. |
| `team:<name>` | The members of a [team](#teams) declared on this registry. |
| `alice` | Exactly that username. Any bare token is a username. |

Tokens are validated at config load and every near-miss is rejected loudly,
rather than being read as a username that happens to admit nobody:

- `$` is reserved for the three built-ins — any other `$token` is an error.
- `all`, `authenticated`, `anonymous`, and `@`-prefixed spellings like `@all`
  are errors suggesting the `$` form.
- `team:` is the only typed prefix; any other `<kind>:` prefix is an error.
  (htpasswd forbids `:` in usernames, which is what frees the character.)
- `team:<name>` must resolve to a team the **same registry** declares — an
  undeclared name is a config error, so a typo can't silently grant to nobody.
- One entry is one token: an entry containing whitespace is an error, so write
  `[alice, bob]` rather than `alice bob`.
- The empty string is an error. To admit nobody, write `[]`; to take the
  default, omit the field.

### Hosted registries

```yaml title="pnpr.yaml"
registries:
  private:
    type: hosted
    org: mycompany
    access: $authenticated
    packages:
      '@mycompany/*':
        publish: $authenticated
        unpublish: $authenticated
      # An explicit entry fully decides its names — this opens one
      # package on an otherwise-private registry.
      '@mycompany/open-sdk':
        access: $all
```

| Key | Description |
| --- | --- |
| `org` | Storage namespace for this registry's packages, so two hosted registries can hold the same `name@version` without colliding. Omitted means the flat `storage` root. Must be a single path-safe segment. |
| `access` | The default read access for `packages:` entries that don't set their own. Defaults to `$all`. This is the only access field settable registry-wide — `publish` and `unpublish` belong to `packages:` entries. |
| `teams` | Named user sets for this registry, referenced from its access lists as `team:<name>` — see [Teams](#teams). |
| `packages` | The registry's namespace and per-package rules — see [above](#the-packages-map). Only these names can be served from or published to this registry. |

Two hosted registries cannot share an `org` namespace — that's rejected at
config load.

The shape of a denial depends on who denied it. A caller that the
registry-level `access:` default also denies is masked with a `404` — a
blanket-private registry never confirms which package names exist. A caller
denied by an explicit `packages:` entry while the default would admit them is
rejected loudly (`401` for anonymous callers, `403` for authenticated ones),
so clients can prompt for credentials.

### Upstream registries

An upstream registry is one external origin — one URL, one credential, one
cache namespace. It is either `public` (fetched anonymously, no credential)
or private (carries a server-owned credential and an `access:` policy naming
who may use it):

```yaml title="pnpr.yaml"
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true

  corp:
    type: upstream
    url: https://npm.corp.example.com/
    auth:
      type: bearer
      token_env: CORP_NPM_TOKEN
    access: $authenticated
    packages:
      '@corp/*': {}
```

| Key | Description |
| --- | --- |
| `url` | Upstream registry URL. |
| `public` | Marks an anonymous, world-readable origin (e.g. the public npm registry). A public upstream sends no credential and no custom headers; declaring `auth`, `access`, or `headers` alongside `public: true` is a config error. |
| `auth` | Server-owned credential for a private origin. `type` is `bearer` or `basic`; provide `token`, or `token_env: true` to read `NPM_TOKEN`, or `token_env: NAME` to read a named environment variable. |
| `headers` | Extra request headers to send upstream. If `headers.Authorization` is set, it overrides the `auth`-derived header. |
| `access` | Which pnpr users may reach this registry at `/~<name>/` (and resolve through it). Required for a non-`public` upstream. |
| `teams` | Named user sets for this registry, referenced from its access lists as `team:<name>` — see [Teams](#teams). |
| `packages` | The names this upstream serves through pnpr — see [above](#the-packages-map). |
| `maxage` | Per-registry packument freshness window. Overrides pnpr's global packument TTL / `--packument-ttl-secs` for this registry. |
| `timeout` | Per-request upstream deadline. Defaults to `30s`. |
| `maxFails` | Consecutive failures before the upstream circuit breaker opens. Defaults to `2`; `0` disables the breaker. |
| `failTimeout` | Cooldown before an open circuit is probed again. Defaults to `5m`. |
| `cache` | Whether tarballs fetched from this registry are cached locally. Defaults to `true`; `false` streams verified tarballs through a temporary file. With caching on, pnpr computes a SHA-512 `dist.integrity` for up to 64 versions per fetched packument that omit one, caching each tarball it hashes. |

Interval values accept strings such as `30s`, `5m`, `1h30m`, or a bare number
of seconds.

Two upstream-specific rules apply to the `packages:` map. Per-package `access`
values are allowed even on a `public: true` upstream — `public` describes the
*fetch* (anonymous, credential-free), while a rule's `access` gates who may
read the name *through pnpr*. And `publish`/`unpublish` values are rejected on
any upstream: no write can land there.

A private upstream registry is more than a proxy: it is a **pnpr-managed
credential** for that origin. One upstream token, held by the server, is
fanned out to a whole team through the registry's `access:` policy — clients
authenticate to pnpr with their own tokens and never see the upstream
credential. Declaring a `packages:` namespace on a private upstream also
bounds what that credential can be used for: a caller can't pull arbitrary
public names through it. pnpr never forwards a client's credentials upstream,
and the server-owned credential is only ever attached over the upstream's own
scheme (an `https://` registry's token is never sent over plain `http://`).

Each upstream gets its own cache namespace: a `public` upstream uses a stable,
secret-free namespace shared across restarts, while a private upstream's cache
is keyed by an HMAC of the registry and its credential — so rotating the
upstream token automatically moves to a fresh namespace, and a private
registry's content can never be served on a public path or through another
registry.

### Routers

A router is an ordered `sources:` list of concrete registries. A package
resolves to the **first listed source whose `packages:` keys claim its name**,
authoritatively — later sources are never consulted, and a name that no source
claims is a definitive 404:

```yaml title="pnpr.yaml"
registries:
  main:
    type: router
    sources: [private, corp, npmjs]
```

A source with no `packages:` map claims every name — the catch-all — and must
be listed last. A router can order competing claims, but it can never assign a
name to a registry that doesn't claim it: the namespace lives on the concrete
registry, and is enforced there on every path.

Because source order is load-bearing — a misordered router is the one way a
mistake could send a private scope to a public origin — pnpr **refuses to
start** on an invalid router:

- a source that is unreachable — everything it claims is already covered by
  earlier sources (including a catch-all that isn't last);
- two sources claiming an identical pattern (genuinely ambiguous provenance,
  rejected in either order);
- a duplicate source;
- a source that is undefined, the router itself, or another router (sources
  must be hosted or upstream registries — no nesting, no cycles);
- a router with no sources.

## Teams {#teams}

A team is a named list of usernames, declared on the registry that uses it and
referenced from that registry's access lists as `team:<name>` — in the
registry-level `access:` default and the per-package `packages:` rules alike:

```yaml title="pnpr.yaml"
registries:
  corp:
    type: upstream
    url: https://npm.corp.example.com/
    auth:
      type: bearer
      token_env: CORP_NPM_TOKEN
    teams:
      platform: [alice, bob]
      frontend: [carol, dave]
    access: team:platform
    packages:
      '@corp/ui':
        access: [team:platform, team:frontend]
```

Teams are **registry-scoped**: a registry can only reference teams it declares
itself, never another registry's. To share one roster between registries, use a
YAML anchor:

```yaml title="pnpr.yaml"
registries:
  private:
    type: hosted
    teams: &corpTeams
      platform: [alice, bob]
    access: team:platform

  corp:
    type: upstream
    url: https://npm.corp.example.com/
    teams: *corpTeams
    access: team:platform
```

A team's members are plain usernames. A team cannot contain another team — an
entry with a `:` in it is a config error.

A team is only ever reached through the `team:` prefix: a bare `platform` in an
access list names the **user** `platform`, even when a team of that name
exists. Keeping the two apart means someone who can register a username can
never inherit a team's grants by choosing its name.

### Managing teams through the API {#managing-teams-through-the-api}

By default the `teams:` map is the roster. npm's team endpoints serve it
read-only and refuse every change. Set `teamsManagedBy: api` on a hosted
registry to let the [`auth.admins`](#auth) edit its roster with `pnpm team`:

```yaml title="pnpr.yaml"
auth:
  admins: [alice]
registries:
  private:
    type: hosted
    teamsManagedBy: api
    teams:
      platform: [alice]
    packages:
      '@corp/*':
        access: [team:platform]
```

- pnpr stores the roster in the hosted store, so every replica serves the same
  one. A replica rereads it at most 10 seconds after another replica changes it.
- If pnpr cannot read a stored roster, that registry's `team:` grants admit
  nobody until a read succeeds. Other access rules are not affected.
- The `teams:` map is used only until the first change is stored. After that,
  editing `teams:` has no effect.
- A team that a `packages:` rule names cannot be destroyed.
- Only hosted registries accept `teamsManagedBy`.

See [Team endpoints](endpoints.md#team-endpoints).

### Changing rules through the API {#changing-rules-through-the-api}

Set `rulesManagedBy: api` on a hosted registry to let the
[`auth.admins`](#auth) change its rules without a config deploy:

```yaml title="pnpr.yaml"
registries:
  private:
    type: hosted
    rulesManagedBy: api
    packages:
      '@corp/*':
        access: $authenticated
```

- An admin can replace the registry-level `access:` and the `access`,
  `publish`, and `unpublish` lists of every key the `packages:` map declares.
  The keys themselves stay in the config, because they decide which registry
  serves a name.
- A change applies at once on the replica that took it. pnpr stores changes in
  the hosted store, so other replicas apply them within 10 seconds.
- Resetting the changes brings back the config's rules.
- If pnpr cannot read the stored changes, the registry admits nobody until a
  read succeeds.
- Only hosted registries accept `rulesManagedBy`.
- `npm access grant` and `npm access revoke` edit the lists of a package the
  `packages:` map declares by name.

See [Admin endpoints](endpoints.md#admin-endpoints) and
[Team package access](endpoints.md#team-package-access).

## `oci`

Size limits for the [container image](container-images.md) surface, and the
Bearer challenge it offers:

```yaml title="pnpr.yaml"
oci:
  bearerAuth: false
  maxBlobBytes: 10737418240   # 10 GiB
  maxManifestBytes: 4194304   # 4 MiB
```

Both limits must be greater than zero.

## `cors`

The exact browser origins allowed to call pnpr across origins. Cross-origin
access stays off when the block is absent or its allowlist is empty. See
[Browsing and discovery](discovery.md).

```yaml title="pnpr.yaml"
cors:
  allowedOrigins:
    - https://registry-ui.example.com
```

## `trustedProxies`

A list of reverse proxy IP addresses or CIDR blocks trusted to forward client IP
addresses in request forwarding headers.

When configured, pnpr evaluates the trusted forwarding header only if the direct socket
peer matches one of the trusted entries. In multi-hop proxy chains, hops are traversed
from right to left, stopping at the first untrusted IP (the real client).
The resolved client IP is recorded on the `pnpr::access` log span and enforced for
CIDR-restricted bearer tokens. When omitted or empty, forwarding headers are ignored
and the socket peer IP is used directly.

Supports exact IPs, CIDR notation (IPv4 and IPv6), and the `"loopback"` keyword.
Accepts `trustedProxies` (camelCase) or `trusted_proxies` (snake_case).

## `trustedProxyHeader`

The specific request header to inspect when behind a trusted proxy:
- `x-forwarded-for` (default): standard comma-separated hops.
- `forwarded`: RFC 7239 `for=...` parameters.
- `x-real-ip`: single client IP header.

To prevent client spoofing, pnpr only inspects the configured header and does not
fall back to other headers.

```yaml title="pnpr.yaml"
trustedProxies:
  - loopback
  - 10.0.0.0/8
  - 192.168.1.100
trustedProxyHeader: x-forwarded-for
```

## `requestTimeout`

The maximum duration that pnpr allows for handling an incoming HTTP request and constructing the response before cancelling it and returning `504 Gateway Timeout`.

Defaults to `60s`. Set to `0` to disable timeouts.

This timeout bounds request processing, authentication, and response generation. Once response headers are sent and streaming of the response body begins (such as for package tarballs or artifact blobs), active body transfers proceed without being interrupted by the timeout.

Accepts `requestTimeout` (camelCase) or `request_timeout` (snake_case).
Can be specified as an integer number of seconds (e.g. `30`) or with a duration suffix (e.g. `30s`, `1m`).

```yaml title="pnpr.yaml"
requestTimeout: 60s
```

## Security limits and protections

pnpr includes built-in protections against denial-of-service and brute-force attacks:

- **Pre-buffer authorization**: Package publish and modification endpoints (`PUT /{pkg}`, `PUT /@{scope}/{pkg}`, `PUT /{pkg}/-rev/{rev}`) check caller authentication and write permissions before reading or buffering request payloads (up to 100 MiB). Unauthorized or unauthenticated requests are rejected immediately with `401 Unauthorized` or `403 Forbidden` without consuming server memory or network bandwidth.
- **Login rate limiting**: Login attempts (`PUT /-/user/org.couchdb.user:...`) are rate-limited to mitigate password brute-force and CPU exhaustion from bcrypt password verification. Failed login attempts are throttled by client IP and by username with a `429 Too Many Requests` response and a `Retry-After` header.
- **Concurrency limits**: Intensive operations are governed by concurrency semaphores to protect against resource exhaustion (limiting concurrent password verifications and concurrent large payload buffering).
- **Upstream request coalescing (singleflight)**: Concurrent requests for the same uncached upstream packument are coalesced so multiple concurrent requests result in only a single upstream fetch.


## `ui`

Whether and from where pnpr serves its web UI. See [Web UI](web-ui.md#settings).

```yaml title="pnpr.yaml"
ui:
  enabled: true
```

## `auth`

By default users are stored in an htpasswd file and tokens in a local SQLite
database:

```yaml title="pnpr.yaml"
auth:
  htpasswd:
    file: ./htpasswd
    # Self-registration is disabled when omitted or set to -1.
    # Set a non-negative cap to allow new users.
    max_users: -1
  tokens:
    # Optional. Defaults to a tokens.db sibling of the htpasswd file.
    file: ./tokens.db
```

Omitted `max_users` means registration disabled, not unlimited registration.
Existing users can still log in. When `auth.tokens.file` is omitted and
`auth.htpasswd.file` is set, the token database defaults to `tokens.db` next to
the htpasswd file.

To share auth state across several stateless pnpr replicas, move users and
tokens into a shared SQL database — see [Auth backends](auth-backends.md).

`auth.admins` lists the usernames that may administer the registry. An admin
may:

- create and remove accounts, change passwords, and revoke tokens through the
  [admin endpoints](endpoints.md#admin-endpoints), even while self-registration
  is disabled;
- edit the teams of every registry with
  [`teamsManagedBy: api`](#managing-teams-through-the-api);
- change the rules of every registry with
  [`rulesManagedBy: api`](#changing-rules-through-the-api).

The first admin account still has to exist. Let it self-register before you
set `max_users` to `-1`, or create it in the htpasswd file or the shared
database.

```yaml title="pnpr.yaml"
auth:
  admins: [alice]
```

An `auth.oidc` list configures OpenID Connect browser sign-in and keyless CI
publishing alongside the password backend — see
[OpenID Connect](oidc.md).

`auth.scim` lets an identity provider deprovision accounts — see
[SCIM deprovisioning](scim.md).

## `secret`

```yaml title="pnpr.yaml"
secret: ${PNPR_SECRET}
```

The HMAC key for pnpr's private cache namespaces — private resolution-cache
entries and private per-registry cache directories are keyed with it, so
on-disk paths reveal neither registry names nor credentials. It must be at
least 16 bytes. When omitted, a fresh random secret is generated per process,
which means private cache entries only survive for that process's lifetime —
set an explicit secret to keep private caches warm across restarts.

## Registry, resolver, artifact, and pipeline surfaces {#registry-resolver-and-artifact-surfaces}

pnpr exposes four independently deployable HTTP surfaces:

- The **registry surface** — package and tarball reads, publish,
  unpublish, dist-tags, and search, on the path-less base and on every
  `/~<name>/` endpoint, for [every ecosystem](ecosystems.md) the config
  declares. It has no config toggle: it is served exactly when at
  least one registry is declared under `registries:`. The CLI flag
  `--disable-registry` turns it off for one process.
- The **resolver surface** — the pnpr install-accelerator routes
  (`GET /-/pnpr`, `POST /-/pnpr/v0/resolve`, and
  `POST /-/pnpr/v0/verify-lockfile`). It is enabled by default; the CLI flag
  `--disable-resolver` overrides the setting:

```yaml title="pnpr.yaml"
resolver:
  enabled: true
```

- The **shared-artifact surface** — the three organization-scoped endpoints of
  the [shared side-effects cache](shared-side-effects-cache.md) proof of
  concept. It is off by default and is a top-level peer of `resolver`, so an
  I/O-bound artifact tier can scale independently of a compute-bound resolver:

```yaml title="pnpr.yaml"
artifacts:
  enabled: true
  orgs:
    acme:
      access: [ci-builder, alice, bob]
      publish: ci-builder
```

  The CLI flag `--disable-artifacts` overrides this setting. With artifacts off,
  its routes are not mounted and the `GET /-/pnpr` handshake advertises an empty
  artifact-version list. The handshake itself is served when either the resolver
  or artifact surface is enabled, so an artifact-only tier is discoverable.

  `orgs` declares the organizations this server keeps build output for, and who
  may read and who may publish what each one owns. An organization owns three
  things:

  - its signed artifacts: [shared side effects](shared-side-effects-cache.md)
    and `pnpm pipeline` task results,
  - its [Cargo compilation cache](compiler-cache.md),
  - its [pipeline run records](pipeline-runs.md).

  `access` and `publish` list pnpr usernames, or `$authenticated` for every
  signed-in account. Teams are declared per registry, so `team:` entries are
  refused here. Publishing also requires `access`. An empty list denies
  everyone. An organization nobody may read is invisible: requests for it answer
  as if it did not exist. An undeclared organization is unavailable.
  `orgs` is read even when `enabled` is false, for a server that only stores
  pipeline runs.

  `quota` bounds what the artifact store keeps, in GiB: `ownerGiB` for one
  owner (an organization, or a cache name), defaulting to 1, and `totalGiB` for
  all of them together, defaulting to 10. A `pnpm pipeline` task result can hold
  release binaries, so a server that stores them usually needs more:

```yaml title="pnpr.yaml"
artifacts:
  quota:
    ownerGiB: 50
    totalGiB: 200
```

- The **pipeline run surface** — the endpoints that store and serve
  [`pnpm pipeline` run records](pipeline-runs.md), a peer of the artifact store.
  It is off by default:

```yaml title="pnpr.yaml"
pipeline:
  enabled: true
```

  Who may read and submit runs is decided by [`artifacts.orgs`](#registry-resolver-and-artifact-surfaces).

Something must be served: a config with no registries (or the registry
surface disabled by flag), the resolver disabled, and artifacts disabled is a
startup error.

The health endpoint (`/-/ping`) and the account endpoints (login, whoami, and
token management) are always served, whichever surfaces are enabled — so
`pnpm login` works even against a resolver-only or artifact-only server. See
[HTTP endpoints](endpoints.md).

## `routes`

Resolver route classification (see
[Install acceleration](install-acceleration.md#private-registries)) treats the
official npm registry as public out of the box. If clients resolve against
other registries that serve anonymously-readable content, declare them as
public routes so their resolutions can be cached and shared across all callers:

```yaml title="pnpr.yaml"
routes:
  public:
    - registry: https://registry.mirror.example.com/
    - registry: https://npm.corp.example.com/
      package: '@oss/*'
```

Each rule may name a `registry` origin, a `package` glob, or both. Rules fail
closed on a typo: a present-but-unparsable `registry` URL or `package` glob
drops the whole rule (with a warning) rather than widening it.

## `osv`

Local OSV checks can hide or reject known vulnerable npm versions without live
OSV API calls:

```yaml title="pnpr.yaml"
osv:
  enabled: true
  path: ./osv/npm/all.zip
```

`path` may point to an OSV npm database zip or an extracted JSON directory. When
omitted, pnpr looks for `<cache>/osv/npm/all.zip`. The same feature can be
enabled from the CLI with `--osv` and `--osv-db`.

## `log`

```yaml title="pnpr.yaml"
log:
  type: stdout
  format: pretty   # or `json`
  level: error     # trace, debug, http, info, warn, or error
```

`RUST_LOG` always overrides the configured level. See the
[CLI reference](cli.md#logging).

### Access logs and request IDs

Every request emits an access log record under the tracing target `pnpr::access` at `info` level:

- `request_id`: incoming `x-request-id` header (if valid printable ASCII, <=128 characters) or a generated RFC 4122 UUID v4. Also echoed in the response `x-request-id` header.
- `method`: HTTP method.
- `uri`: request path and query string.
- `client_ip`: client IP resolved according to [`trustedProxies`](#trustedproxies) and [`trustedProxyHeader`](#trustedproxyheader).
- `user`: authenticated username, or `"-"` when unauthenticated.
- `user_agent`: `User-Agent` header value, or `"-"` when omitted.
- `status`: HTTP response status code.
- `latency_ms`: request round-trip processing duration in milliseconds.
- `bytes`: response body size in bytes, derived from `Content-Length` (0 for chunked or streamed responses without length headers).

### Admin audit logs

Administrative and destructive operations emit structured audit records under the tracing target `pnpr::audit` at `info` level. When structured logging is enabled (`format: json`), these events output as JSON objects that inherit the active request span context, including `request_id`, `client_ip`, `method`, and `uri`.

Audit events record the `action`, the `actor` (authenticated username, bearer token identifier, or client identity), and operation-specific attributes:

- **User management**:
  - `user.create`: creating an account (`actor`, `user`).
  - `user.password_update`: changing an account password (`actor`, `user`).
  - `user.token_revoke`: revoking user tokens (`actor`, `user`).
  - `user.delete`: deleting an account (`actor`, `user`).
  - `user.register`: self-registering a new account (`actor`, `user`).
- **Rule overrides**:
  - `rules.replace`: replacing dynamic package access rules (`actor`, `registry`).
  - `rules.delete`: removing dynamic package access rules (`actor`, `registry`).
- **Team administration and package access**:
  - `team.create`: creating a team (`actor`, `scope`, `team`).
  - `team.destroy`: deleting a team (`actor`, `scope`, `team`).
  - `team.member_add`: adding a user to a team (`actor`, `scope`, `team`, `user`).
  - `team.member_remove`: removing a user from a team (`actor`, `scope`, `team`, `user`).
  - `team.access_grant`: granting package permissions to a team (`actor`, `scope`, `team`, `package`, `permissions`).
  - `team.access_revoke`: revoking package permissions from a team (`actor`, `scope`, `team`, `package`).
- **SCIM provisioning**:
  - `scim.user_create`: provisioning a SCIM user (`actor`, `user`, `active`).
  - `scim.user_replace`: replacing SCIM user attributes (`actor`, `user`, `active`).
  - `scim.user_patch`: updating SCIM user status (`actor`, `user`, `active`).
  - `scim.user_delete`: deprovisioning a SCIM user (`actor`, `user`).
- **Package unpublishing and mutations**:
  - `package.unpublish`: unpublishing an entire package (`actor`, `package`).
  - `package.unpublish_tarball`: unpublishing a specific package version tarball (`actor`, `package`, `tarball`).
  - `package.update_packument`: modifying packument metadata directly (`actor`, `package`).
  - `package.dist_tag_set`: assigning a dist-tag (`actor`, `package`, `tag`, `version`).
  - `package.dist_tag_remove`: deleting a dist-tag (`actor`, `package`, `tag`).
  - `oci.delete_blob`: deleting an OCI blob (`actor`, `repository`, `digest`).
  - `oci.delete_manifest`: deleting an OCI manifest (`actor`, `repository`, `reference`).

### Prometheus metrics

pnpr serves Prometheus metrics at `GET /-/metrics` (and `GET /metrics` for scrapers) in standard Prometheus text format (`text/plain; version=0.0.4`). Requests to `GET /metrics` carrying npm packument `Accept` headers receive the `metrics` package packument instead. Like health endpoints (`/-/ping`, `/-/ready`, `/-/live`), the metrics endpoints are intentionally unauthenticated to support telemetry scrapers:

- `pnpr_http_requests_in_flight` (gauge): number of requests currently being handled.
- `pnpr_http_requests_total` (counter): requests served, labelled by `method` (normalized to standard HTTP methods or `OTHER`) and `status`.
- `pnpr_http_request_duration_seconds` (histogram): request duration distribution across buckets from 0.005s to 10.0s.
- `pnpr_http_response_bytes_total` (counter): total response payload bytes sent, derived from `Content-Length`.
- `pnpr_cache_hits_total` (counter): packument cache hits on upstream proxies.
- `pnpr_cache_misses_total` (counter): packument cache misses requiring upstream fetches.
- `pnpr_login_rate_limits_total` (counter): login attempts throttled by rate limiting.
- `pnpr_build_info` (gauge): server build version info.

## Environment variable substitution

Any `${ENV_VAR}` in the config is substituted from the environment before
parsing, so secrets can be kept out of the file:

```yaml title="pnpr.yaml"
s3:
  bucket: my-pnpr-packages
  accessKeyId: ${PNPR_S3_ACCESS_KEY_ID}
  secretAccessKey: ${PNPR_S3_SECRET_ACCESS_KEY}
```
