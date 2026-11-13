---
id: configuration
title: Configuration
---

pnpr uses a YAML config. When you don't pass `-c`, pnpr looks for a global
`config.yaml` in its config directory and otherwise falls back to a minimal
production default.

The config file holds the server settings. The registries pnpr serves live in
the [database](#registries-in-the-database). On its first start, pnpr
stores a default set: the public npm registry and a hosted registry for
private packages.

A minimal config:

```yaml title="pnpr.yaml"
storage: ./storage
secret: ${PNPR_SECRET}
```

To host a private scope locally and route everything else to the public npm
registry, describe the registries in a file of their own:

```yaml title="registries.yaml"
registries:
  local:
    type: hosted
    packages:
      # The names this registry serves, plus their access rules.
      '@mycompany/*':
        access: $authenticated

  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true

  main:
    type: router
    sources: [local, npmjs]

defaultRegistry: main
```

Pass the config with `-c`, import the registries with
[`pnpr registries import`](cli.md#registries), and create the first account
with [`pnpr add-user`](cli.md#add-user). Generate `PNPR_SECRET` once, store it
securely, and supply the same value on every restart:

```sh
export PNPR_SECRET="$(openssl rand -hex 32)"
pnpr -c ./pnpr.yaml registries import ./registries.yaml
pnpr -c ./pnpr.yaml add-user alice
pnpr -c ./pnpr.yaml
```

Relative paths in the config, such as `storage`, `cache`, `osv.path`, and
`ui.dir`, resolve against the config file's directory. For the bundled config,
they resolve against the working directory.

## `listen` and `publicUrl`

```yaml title="pnpr.yaml"
listen: 0.0.0.0:7677
publicUrl: https://registry.example.com
```

`listen` is the address pnpr binds to. It defaults to `127.0.0.1:7677`.
`publicUrl` is the URL clients use to reach pnpr. pnpr rewrites the
`dist.tarball` URLs in served packuments onto it. It defaults to
`http://<listen>`, and pnpr logs a warning at startup if `listen` is not a
loopback address and `publicUrl` is unset. The `--listen` and `--public-url`
flags override both.

## `storage` and `cache`

pnpr keeps two kinds of data:

- **`storage`**: the source of truth. It holds packages published to this server plus
  anything served in static mode. Back this up and keep it on a durable volume.
- **`cache`**: the disposable mirror of upstream registries plus the resolver
  store, resolver cache, lockfile-verdict cache, local shared-artifact store,
  and S3 upload staging scratch.
  Safe to wipe at any time. Defaults to a `.pnpr-cache` subdirectory of
  `storage`; point it at a separate, ephemeral volume to keep cached upstream
  content off the durable disk.

```yaml title="pnpr.yaml"
# Path string:
storage: ./storage

# Or settings table that also names the object store and the database:
#storage:
#  path: ./storage   # defaults to ./storage
#  s3:               # see Storage backends
#    bucket: my-pnpr-packages
#  database:         # see The database
#    url: libsql://my-db.turso.io

# Path string:
#cache: ./cache

# Or settings table with budget and retention policies:
#cache:
#  path: ./cache     # defaults to .pnpr-cache in storage
#  maxSize: 20GB     # enforces LRU eviction when total size exceeds this
#  maxAge: 30d       # TTL deletion of cached entries older than this
```

`maxSize` and `maxAge` cover the whole cache directory, including upstream
mirrors, tar variants, the resolver store, the resolver cache, and the
lockfile-verdict database. When either is configured, pnpr evicts stale entries
on startup and runs periodic eviction in the background. On startup, pnpr also
prunes orphaned upstream namespaces from removed registries or rotated secrets.
Use [`pnpr cache-gc`](cli.md#cache-gc) for on-demand or offline reclamation.

`storage` takes the storage directory, or a table with these keys:

| Field | Meaning |
| --- | --- |
| `path` | The storage directory. Defaults to `./storage`. [`--storage`](cli.md#options) overrides it. |
| `s3` | Keeps the hosted store in an S3-compatible object store. See [Storage backends](storage.md#backends). |
| `database` | Names the database for accounts, tokens, and package metadata. See [The database](storage.md#database). |

With the filesystem hosted store, package metadata lives in an embedded SQLite
database by default. A server that hosts packages in `storage.s3` needs a
[shared database](auth-backends.md) instead.

## `registries` and `defaultRegistry`

The `registries:` map of the [registries document](#registries-in-the-database)
declares every registry pnpr serves. Every addressable
registry origin is a named registry, exposed at `https://<pnpr>/~<name>/`. There
are three kinds:

- **`hosted`**: a registry pnpr itself is the authoritative origin for. The
  only kind that accepts writes (publish, dist-tags, unpublish).
- **`upstream`**: exactly one external registry origin, proxied and cached.
- **`router`**: an ordered list of concrete source registries; a package
  resolves to the first listed source that claims its name.

The model is governed by one invariant: **provenance is declared, never
inferred**. Every concrete registry declares the package names it claims through
its [`packages:` map](#the-packages-map), a package
resolves to exactly one declared origin, and no configuration can express a
cross-origin fall-through, not on "not found" and not on "unavailable". Claimed
package patterns are enforced at the registry, on every path to it: a read of an
unclaimed name is a definitive 404 answered before storage or the
upstream is consulted, and a publish of an unclaimed name is rejected.

A router closes the dependency-confusion class of attacks for every client that
resolves through it. Each registry checks only its own claims, so a request
addressed to an upstream's own `/~<name>/` URL gets any name that upstream
claims, including a name in a scope a hosted registry claims. Point clients at
a router, not at an upstream.

`defaultRegistry` names the registry that an ecosystem's base URL, such as
`https://<pnpr>/npm/`, aliases. This is usually a router. When omitted, the base serves
no registry and clients must address a `/~<name>/` URL under it.

A registry name is served as the URL path segment `/~<name>/`. It must start
with a letter and contain only letters, digits, `.`, `_`, and `-`. It cannot be
one of pnpm's
[reserved prefix names](https://pnpm.io/settings/dependency-resolution#reserved-alias-names), such as
`npm`, `git`, `github`, or `workspace`. These are the rules for a pnpm
[registry prefix](https://pnpm.io/registries#prefix), so a registry name can be
used as one. The `~` must arrive unencoded: `%7E` does not select the named
registry.

A registry also declares which package ecosystem it
serves, through `ecosystem:` (`npm`, `cargo`, `pypi`, or `oci`; npm by default).
Each ecosystem is addressed through its own path prefix (`/npm/`, `/cargo/`,
`/pypi/`, or `/oci/`). Declare registries directly under `registries:` with
unique names. See
[Cargo and Python registries](ecosystems.md) and
[Container images](container-images.md).

### The `packages` map

Every concrete registry (hosted or upstream) takes an optional `packages:`
map. Its **keys are the package names the registry claims**,
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

Any other wildcard is refused rather than kept as a silently-never-matching
literal.

The **most specific matching key wins**: an exact name beats `@scope/*`, which
beats `@*/*`, which beats `**`. Key order carries no meaning. At most one key
per specificity tier can match a name, so the winning rule is unique and
reordering the map (by a formatter, or a YAML round-trip) can never change
which rule applies. A duplicate key is refused.

Each value can set `access`, `publish`, and `unpublish`. When a field is
omitted, `access` falls back to the registry-level `access:` default,
`publish` defaults to `$owners`, and `unpublish` defaults to nobody.
Cargo and PyPI registries reject `unpublish` rules because those ecosystems do
not support package unpublishing.

Every one of those fields takes either a single access token or a YAML list of
them, and the grammar is deliberately small:

| Token | Admits |
| --- | --- |
| `$all` | Anyone, authenticated or not. |
| `$authenticated` | Any logged-in user. |
| `$anonymous` | Callers with no credentials. |
| `$owners` | The package's owners and the [`auth.admins`](#auth). The first publish of a new name makes the publisher its owner. An npm package's owners are its maintainers. Valid only in `publish` and `unpublish` lists. |
| `team:<id>` | The members of a [team](#teams) declared on this registry. |
| `<account-id>` | Exactly that account ID. Bare tokens are account IDs. |

Tokens are validated when the registries are stored:

- `$` is reserved for the built-ins above. Any other `$token` is an error.
- `all`, `authenticated`, `anonymous`, and `@`-prefixed spellings like `@all`
  are errors suggesting the `$` form.
- `team:` is the only typed prefix; any other `<kind>:` prefix is an error.
  (IDs cannot contain `:`.)
- `team:<id>` must resolve to a team the **same registry** declares. An
  undeclared team ID is refused, so a typo can't silently grant to nobody.
- One entry is one token: an entry containing whitespace is an error, so write
  `[<account-id-1>, <account-id-2>]` rather than `<account-id-1> <account-id-2>`.
- The empty string is an error. To admit nobody, write `[]`; to take the
  default, omit the field.

### Hosted registries

```yaml title="registries.yaml"
registries:
  private:
    type: hosted
    namespace: mycompany
    access: $authenticated
    packages:
      '@mycompany/*':
        unpublish: $owners
      # An explicit entry fully decides its names — this opens one
      # package on an otherwise-private registry.
      '@mycompany/open-sdk':
        access: $all
```

| Key | Description |
| --- | --- |
| `namespace` | [Storage namespace](storage.md#layout) for this registry's packages, so two hosted registries can hold the same `name@version` without colliding. Omitted means the default namespace. Must be a single path-safe segment. |
| `access` | The default read access for `packages:` entries that don't set their own. Defaults to `$all`. This is the only access field settable registry-wide. `publish` and `unpublish` belong to `packages:` entries. |
| `teams` | Named user sets for this registry, referenced from its access lists as `team:<id>`. See [Teams](#teams). |
| `packages` | Claimed package patterns and per-package rules. See [above](#the-packages-map). Only these names can be served from or published to this registry. |
| `tarballs` | The format this registry serves tarballs in: `tgz` (default) or `tar`. See [Tar registries](#tar-registries). |

Two hosted registries of the same ecosystem cannot share a `namespace`.
pnpr refuses to store them. Hosted registries of different ecosystems can
share a `namespace` because each ecosystem uses its own `<ecosystem>/` directory
inside the namespace.

The shape of a denial depends on who denied it. A caller that the
registry-level `access:` default also denies is masked with a `404`. A
blanket-private registry never confirms which package names exist. A caller
denied by an explicit `packages:` entry while the default would admit them is
rejected loudly (`401` for anonymous callers, `403` for authenticated ones),
so clients can prompt for credentials.

### Upstream registries

An upstream registry is one external origin: one URL, one credential, one
cache namespace. It is either `public` (fetched anonymously, no credential)
or private (carries a server-owned credential and an `access:` policy naming
who may use it):

```yaml title="registries.yaml"
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
      tokenEnv: CORP_NPM_TOKEN
    access: $authenticated
    packages:
      '@corp/*': {}
```

| Key | Description |
| --- | --- |
| `url` | Upstream registry URL. |
| `public` | Marks an anonymous, world-readable origin (e.g. the public npm registry). A public upstream sends no credential and no custom headers; declaring `auth`, `access`, or `headers` alongside `public: true` is refused. |
| `auth` | Server-owned credential for a private origin. `type` is `bearer` or `basic`; provide `token` as a `${NAME}` reference, or `tokenEnv: true` to read `NPM_TOKEN`, or `tokenEnv: NAME` to read a named environment variable. See [Upstream credentials](#upstream-credentials). |
| `headers` | Extra request headers to send upstream, each value a `${NAME}` reference. If `headers.Authorization` is set, it overrides the `auth`-derived header. |
| `access` | Which pnpr users may reach this registry at `/~<name>/` (and resolve through it). Required for a non-`public` upstream. |
| `teams` | Named user sets for this registry, referenced from its access lists as `team:<id>`. See [Teams](#teams). |
| `packages` | The names this upstream serves through pnpr. See [above](#the-packages-map). |
| `packumentTtl` | Per-registry packument freshness window. Overrides [`packumentTtl`](#packumentttl) and `--packument-ttl` for this registry. |
| `timeout` | How long pnpr waits for the upstream to answer, and for more of a tarball it is downloading. A download that keeps receiving data runs to the end. A metadata request must finish within it. Defaults to `30s`. |
| `maxFails` | Consecutive failures before the upstream circuit breaker opens. Defaults to `2`; `0` disables the breaker. |
| `maxPackumentSize` | Maximum upstream npm package metadata response size. Defaults to `64 MiB`. Accepts a positive number of bytes or a size such as `64 MiB`. |
| `failTimeout` | Cooldown before an open circuit is probed again. Defaults to `5m`. |
| `tarballs` | The format this registry serves tarballs in: `tgz` (default) or `tar`. See [Tar registries](#tar-registries). |
| `search` | Opts this upstream into browser-facing search and organization discovery. Defaults to `false`. See [Browsing and discovery](discovery.md). |
| `cache` | Whether tarballs fetched from this registry are cached locally. Defaults to `true`; `false` streams verified tarballs through a temporary file. With caching on, pnpr computes missing SHA-512 `dist.integrity` values in the background for up to 64 versions per fetched packument. It reuses cached tarballs and retains matching integrities across refreshes. Initial metadata responses may omit these integrities until the work completes. Downloads that need a computed integrity wait for scheduled backfill, up to the upstream `timeout`. Failed pins are retried on the next metadata refresh. Backfill work waits for capacity and retries when metadata changes during a download. At most 256 package jobs can be queued or active. When that limit is reached, metadata still responds, but downloads that need a computed integrity return HTTP 429 until capacity is available. Computing integrities does not extend metadata freshness. |

Tarball requests refresh stale package metadata before serving cached bytes.
A refresh deletes cached tarballs for versions the upstream removed.

Timeouts while reading a response body do not count toward `maxFails`.
An open circuit returns HTTP 503 with the configured upstream name and a
`Retry-After` header indicating the remaining cooldown in seconds. Cached
packages remain available.

Interval values accept strings such as `30s`, `5m`, `1h30m`, or a bare number
of seconds.

Two upstream-specific rules apply to the `packages:` map. Per-package `access`
values are allowed even on a `public: true` upstream. `public` describes the
*fetch* (anonymous, credential-free), while a rule's `access` gates who may
read the name *through pnpr*. And `publish`/`unpublish` values are rejected on
any upstream: no write can land there.

A private upstream registry is more than a proxy: it is a **pnpr-managed
credential** for that origin. One upstream token, held by the server, is
fanned out to a whole team through the registry's `access:` policy. Clients
authenticate to pnpr with their own tokens and never see the upstream
credential. Declaring a `packages:` map on a private upstream also
bounds what that credential can be used for: a caller can't pull arbitrary
public names through it. pnpr never forwards a client's credentials upstream,
and the server-owned credential is only ever attached over the upstream's own
scheme (an `https://` registry's token is never sent over plain `http://`).

Each upstream gets its own cache namespace: a `public` upstream uses a stable,
secret-free namespace shared across restarts, while a private upstream's cache
is keyed by an HMAC of the registry and its credential, so rotating the
upstream token automatically moves to a fresh namespace, and a private
registry's content can never be served on a public path or through another
registry.

### Routers

A router is an ordered `sources:` list of concrete registries. A package
resolves to the **first listed source whose `packages:` keys claim its name**,
authoritatively. Later sources are never consulted, and a name that no source
claims is a definitive 404:

```yaml title="registries.yaml"
registries:
  main:
    type: router
    sources: [private, corp, npmjs]
```

A source with no `packages:` map claims every name (the catch-all) and must
be listed last. A router can order competing claims, but it can never assign a
name to a registry that doesn't claim it: claimed package patterns live on the
concrete registry, and are enforced there on every path.

Because source order is load-bearing (a misordered router is the one way a
mistake could send a private scope to a public origin), pnpr **refuses to
store** an invalid router:

- a source that is unreachable: everything it claims is already covered by
  earlier sources (including a catch-all that isn't last);
- two sources claiming an identical pattern (genuinely ambiguous provenance,
  rejected in either order);
- a duplicate source;
- a source that is undefined, the router itself, or another router (sources
  must be hosted or upstream registries; no nesting, no cycles);
- a router with no sources.

A router also takes [`tarballs`](#tar-registries).

### Tar registries

A registry with `tarballs: tar` serves each version as an uncompressed tar, the gzip-decoded content of its `.tgz`, with HTTP content coding the client's `Accept-Encoding` allows. Its metadata describes the tars: `dist.integrity` is the hash of the tar, and `dist.tarball`, `dist.shasum`, and `dist.signatures` are left out. A version with [registry revisions](https://pnpm.io/registry-revisions) keeps its `dist.tarball` on the `<name>@<version>+r<revision>` route, its `dist.revision`, and its `dist.revisions` history, because a revision is always served as its uncompressed tar. A tool that verifies npm signatures through its registry, such as `pnpm/setup`, needs a `.tgz` registry.

To offer tars next to `.tgz` files, add a router over the same sources:

```yaml title="registries.yaml"
registries:
  local:
    type: hosted
    packages:
      '@acme/*':
        publish: $authenticated
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  main:
    type: router
    sources: [local, npmjs]
  zstd:
    type: router
    sources: [local, npmjs]
    tarballs: tar

defaultRegistry: main
```

npm, Yarn, and pnpm projects that do not opt in keep using `main`. A pnpm project opts in by keying `https://<pnpr>/npm/~zstd/` with `serverType: pnpr` in its [`registries`](https://pnpm.io/registries#tar-registries).

- Every registry serves a version's tarball at `<registry>/<name>@<version>`. A tar registry serves no `.tgz` path.
- For an upstream package without fresh cached metadata, pnpr reads the version's metadata from `<upstream>/<name>/<version>`. If the upstream does not serve it, pnpr reads the package's full metadata.
- A hosted registry that declares `tarballs: tar` keeps no `.tgz`. At publish, pnpr compresses the tar at level 19 and stores only that. A `zstd` client gets the stored bytes, and any other client gets the tar decoded, compressed as the request accepts. pnpr refuses to store a router without `tarballs: tar` that lists such a registry as a source, because the router would have no `.tgz` to serve. npm signatures and provenance sign the `.tgz`, so these packages have neither.
- For every other registry, pnpr keeps the published `.tgz` and derives the tar from it:
  - A `gzip` client gets the `.tgz` itself, which is the gzip encoding of its tar.
  - A `zstd` client gets a level-19 encoding once pnpr has stored one. Until then pnpr compresses the tar at level 3 as it streams, and a background worker stores the level-19 encoding. Tars larger than 256 MiB stay at level 3. The level-19 encodings live in the cache, so they can be evicted and are made again.
  - Any other client gets the tar uncompressed.
- A hosted registry learns a tar's hash at publish. An upstream registry learns it the first time it serves the tar with `zstd` or no compression, after verifying the upstream `.tgz`. Until then the version's metadata carries no `dist.integrity`, and pnpm hashes the tar it downloads.
- Tar hashes are kept in the hosted storage under `.tar-integrity/`, which every replica shares.
- pnpr decodes at most 4.25 GiB from one `.tgz`, the most pnpm accepts by default. A version past that has no tar. Copies of tars waiting for the level-19 encoding share a 1 GiB quota, and a request makes no copy when it is full or when another request is already copying that version.
- `tarballs` is accepted only on npm registries.

You can also set `tarballs: tar` on the main router, so the base every pnpm project already uses serves tars. That works when all your clients are pnpm 12.15 or newer and every project declares the base with `serverType: pnpr` in its [`registries`](https://pnpm.io/registries#tar-registries). Without that declaration, pnpm fails with "names no tarball URL". npm, Yarn, and older pnpm cannot install from such a base: it has no `.tgz` path, and its metadata has no tarball URL, except for versions with [registry revisions](https://pnpm.io/registry-revisions). A project whose lockfile was written against the `.tgz` base has `.tgz` hashes in it, so run `pnpm install --update-checksums` there once after the switch.

## Teams {#teams}

A team has a stable ID, a mutable name, and a list of account IDs. It is declared on the registry that uses it and
referenced from that registry's access lists as `team:<id>`, in the
registry-level `access:` default and the per-package `packages:` rules alike:

```yaml title="registries.yaml"
registries:
  corp:
    type: upstream
    url: https://npm.corp.example.com/
    auth:
      type: bearer
      tokenEnv: CORP_NPM_TOKEN
    teams:
      <platform-team-id>: { name: platform, members: [<account-id-1>, <account-id-2>] }
      <frontend-team-id>: { name: frontend, members: [<account-id-3>, <account-id-4>] }
    access: team:<platform-team-id>
    packages:
      '@corp/ui':
        access: [team:<platform-team-id>, team:<frontend-team-id>]
```

Teams are **registry-scoped**: a registry can only reference teams it declares
itself, never another registry's. To share one roster between registries, use a
YAML anchor:

```yaml title="registries.yaml"
registries:
  private:
    type: hosted
    teams: &corpTeams
      <platform-team-id>: { name: platform, members: [<account-id-1>, <account-id-2>] }
    access: team:<platform-team-id>

  corp:
    type: upstream
    url: https://npm.corp.example.com/
    teams: *corpTeams
    access: team:<platform-team-id>
```

A team's members are account IDs. A team cannot contain another team. An
entry with a `:` in it is refused.

The keys under `teams:` are stable team IDs. Keep a key when changing its `name`.
Use a new key for a replacement team. Access lists and OIDC group grants refer
to that key. A bare access token refers to an account ID.

### Managing teams through the API {#managing-teams-through-the-api}

The [`auth.admins`](#auth) and `auth.registryAdmins` can edit the teams of a
hosted npm registry with `pnpm team`.

- A change applies at once on the replica that took it. Other replicas follow
  the [replica reload timing](#replica-reloads).
- A team that a `packages:` rule names cannot be destroyed.
- The team endpoints edit only npm hosted registries, because team management
  is specific to the npm org and team API. To change the teams of another
  registry, replace its entry through the
  [registry admin endpoints](endpoints.md#registry-admin-endpoints).

See [Team endpoints](endpoints.md#team-endpoints).

### Changing rules through the API {#changing-rules-through-the-api}

The [`auth.admins`](#auth) and `auth.registryAdmins` can change the rules of a
hosted registry through the API:

- An admin can replace the registry-level `access:` and the `access`,
  `publish`, and `unpublish` lists of every key the `packages:` map declares.
  Adding or removing a key changes which registry serves a name, so only the
  `auth.registryAdmins` can do it, by replacing the registry's entry.
- A change applies at once on the replica that took it. Other replicas follow
  the [replica reload timing](#replica-reloads).
- A shared cache in front of pnpr, such as a CDN, can keep serving a package
  that a change makes private. A packument stays until its `max-age`, the
  [`packumentTtl`](#packumentttl), runs out. A tarball carries no `max-age`, so
  it stays for as long as the cache keeps it.
- To undo a change, restore an earlier revision.
- `npm access grant` and `npm access revoke` edit the lists of a package the
  `packages:` map declares by name.

See [Admin endpoints](endpoints.md#admin-endpoints) and
[Team package access](endpoints.md#team-package-access).

## Registries in the database {#registries-in-the-database}

pnpr keeps the registries in the
[database](storage.md#database). Every replica serves the same
registries, and admins change them through the
[registry admin endpoints](endpoints.md#registry-admin-endpoints) without a
config deploy. The stored registries are a registries document: the
`registries`, `defaultRegistry`, and `routes.public` keys this page describes,
in a file of their own.

```yaml title="pnpr.yaml"
storage: ./storage
secret: ${PNPR_SECRET}
registryEnv: [CORP_NPM_TOKEN, CORP_API_KEY]
auth:
  admins: [<account-id>]
  registryAdmins: [<account-id>]
```

The config file holds the server settings, and the database holds the
registries:

| In the database | In the config file |
| --- | --- |
| `registries`, with their teams and `packages:` rules | `storage`, `cache`, `secret` |
| `defaultRegistry` | `auth`, including `auth.admins` and `auth.registryAdmins` |
| `routes.public` | `routes.allowedPrivateNetworks` |
| | `registryEnv`, and every other setting on this page |

A config file that declares `registries`, `defaultRegistry`, or
`routes.public` is refused at startup, and so is an
[`artifacts.owners`](#registry-resolver-and-artifact-surfaces) list that names
a `team:`. To move registries out of a config file, copy those blocks into a
file of their own, remove them from the config, and import the file with
[`pnpr registries import`](cli.md#registries). `pnpr registries export`
writes the stored registries back out in the same shape.

On its first start against an empty database, pnpr stores a default set of
registries:

- `npmjs`, the public npm registry, which is also the `defaultRegistry`;
- `local`, a hosted registry whose packages only signed-in users may read.

pnpr stores the defaults once, as the first revision. When several replicas
start at the same time, one of them stores the defaults and the others load
them. Later changes and imports replace them like any other revision.

The registry surface is served even while no registry is stored. A server
started with `--disable-registry`, which serves only the resolver, also reads
the stored registries, so it needs the database too.

### Reviewed changes

To change the registries only through a reviewed file, keep the registries
document in version control and leave `auth.registryAdmins` empty, so nobody
can change the registries through the API. After a change is reviewed, CI
imports the file:

```sh
pnpr -c ./pnpr.yaml registries import ./registries.yaml
```

`pnpr registries import --check` compares the file with the stored registries
without changing them. It lists what differs and exits with a nonzero code when
they differ, so a scheduled CI job can detect a change made outside the
reviewed file. The `auth.admins` can still edit teams and package access
lists, and `--check` reports those edits too.

### Who may change the registries

- `auth.registryAdmins` may change everything: add, replace, and remove
  registries, change `defaultRegistry` and `routes.public`, and restore an
  earlier revision.
- `auth.admins` may read the stored registries and their revisions, edit
  teams, and change the access lists of declared `packages:` keys. An admin
  who is not a registry admin cannot change which registry serves a name or
  where an upstream sends its credential.

Each change is stored as a new revision and logged under
[`pnpr::audit`](#admin-audit-logs). pnpr keeps the newest 500 revisions.

### Upstream credentials

The database stores no credential values. An upstream names the environment
variables that hold its credential, and each replica reads them when it loads
the registries:

```yaml title="registries.yaml"
registries:
  corp:
    type: upstream
    url: https://npm.corp.example.com/
    auth:
      type: bearer
      tokenEnv: CORP_NPM_TOKEN
    headers:
      X-Api-Key: ${CORP_API_KEY}
    access: $authenticated
```

- `auth.tokenEnv` and `${NAME}` references in `auth.token` and in `headers`
  values may name only the variables that `registryEnv` lists in the config
  file. `tokenEnv: true` names `NPM_TOKEN`.
- `auth.token` and each `headers` value must be a single `${NAME}`
  reference. A literal value is refused, so put a header that is not a
  secret in a variable too.
- No other field takes `${NAME}` references.
- An upstream `url` or a `routes.public` registry with a user name, password,
  or query string in it is refused.
- Every replica needs every variable a stored upstream names. A replica
  without one keeps serving the registries it loaded before and logs an error,
  and a change it takes is refused.

### Upstream addresses

An upstream cannot reach loopback, private, or link-local addresses, the same
rule the resolver follows. A `url` that names
one, or `localhost`, is refused, and a hostname that resolves to one is not
connected to. To use an upstream on an internal network, list its addresses in
[`routes.allowedPrivateNetworks`](#routes). An upstream on pnpr's own
`publicUrl` host is refused at any port, even when that host is a public IP.
When `publicUrl` names a hostname, pnpr does not look up its address, so an
upstream written as that address is checked by the rules above.

### Changes pnpr refuses

A change is refused with `400` when the registries it would store do not load.
pnpr also refuses with `409` to:

- remove or rename a hosted registry that holds packages, or move it to
  another `namespace` or `ecosystem`;
- remove a registry or team that an [OIDC](oidc.md) workload or group grant
  names;
- destroy a team that a `packages:` rule names.

A registry is identified by its name. Renaming one removes it and adds a new
one.

Removing a hosted registry, or moving it to another `namespace` or
`ecosystem`, retires the namespace it served. A replica that has not
[reloaded](#replica-reloads) the change still routes publishes there, and pnpr
refuses them with `503` and the error code `ERR_PNPR_REGISTRIES_UNAVAILABLE`.
A retry reaches the reloaded registries. A hosted registry that serves the
namespace again lifts the refusal.

## Replica reloads {#replica-reloads}

The [stored registries](#registries-in-the-database) and the
[SCIM directory](scim.md) reload when a request arrives after the replica's
state is 10 seconds old. That request starts a background reload and can still
use the existing state, so a change can take effect later than 10 seconds after
it is made. The first request after startup waits for the initial reload.
A request that arrives after the registries have gone 60 seconds without a
reload also waits for one, at most five seconds. A change applies at once on the
replica that made it.

Each record read has a five-second deadline. The registries and the SCIM
directory reload independently:

- If the stored registries cannot be read or loaded, the replica keeps
  serving the registries it loaded last. When it has not loaded them for
  60 seconds, or has never loaded them, it answers requests with `503` and the
  error code `ERR_PNPR_REGISTRIES_UNAVAILABLE`, and
  [`/-/ready`](endpoints.md#always-available) fails. The health probes, the
  account and sign-in endpoints, and the
  [registry admin endpoints](endpoints.md#registry-admin-endpoints) keep
  answering, so an admin can restore a revision that loads. A replica whose
  stored registries cannot be loaded at startup still starts.
- If the SCIM directory cannot be read, the replica refuses user credentials.

A successful read restores the stored state. After any failed read, pnpr
retries on a request arriving at least one second after the reload finishes.
Reloads share a lock with management writes. These read deadlines do not
cancel writes. A request that finds the registries 60 seconds old waits at most
five seconds for a management write to release the lock, then answers `503`.

## `oci`

Size limits for the [container image](container-images.md) surface, and the
Bearer challenge it offers:

```yaml title="pnpr.yaml"
oci:
  bearerAuth: false
  maxBlobSize: 10GiB
  maxManifestSize: 4MiB
```

The values shown are the defaults. Both limits must be greater than zero. See
[Container images](container-images.md#authentication) for `bearerAuth`.

`maxChunkSize` optionally advertises a positive chunk size through
`OCI-Chunk-Max-Length` on upload start and progress responses. It is a hint,
not an enforced request limit, and does not change `maxBlobSize`, which caps
the whole blob. The header is an [OCI proposal](https://github.com/opencontainers/distribution-spec/issues/485)
and requires client support. See [Cloudflare](cloudflare.md) for proxy limits.

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

The reverse proxies trusted to forward client IP addresses in request headers.

```yaml title="pnpr.yaml"
trustedProxies:
  networks:
    - loopback
    - 10.0.0.0/8
    - 192.168.1.100
  header: x-forwarded-for
```

`networks` lists the proxies' IP addresses or CIDR blocks. It supports exact IPs,
CIDR notation (IPv4 and IPv6), and the `"loopback"` keyword. pnpr evaluates the
forwarding header only if the direct socket peer matches one of them. In
multi-hop proxy chains, hops are traversed from right to left, stopping at the
first untrusted IP (the real client). The resolved client IP is recorded on the
`pnpr::access` log span and enforced for CIDR-restricted bearer tokens. When
`networks` is omitted or empty, forwarding headers are ignored and the socket
peer IP is used directly.

`header` is the request header to inspect:

- `x-forwarded-for` (default): standard comma-separated hops.
- `forwarded`: RFC 7239 `for=...` parameters.
- `x-real-ip`: single client IP header.

To prevent client spoofing, pnpr only inspects the configured header and does not
fall back to other headers.

## `requestTimeout`

The maximum duration that pnpr allows for handling an incoming HTTP request and constructing the response before cancelling it and returning `504 Gateway Timeout`.

Defaults to `60s`. Set to `0` to disable timeouts.

This timeout bounds request processing, authentication, and response generation. Once response headers are sent, artifact transfers such as package tarballs and shared-artifact blobs proceed without being interrupted by the timeout. Streaming resolver responses (`POST /-/pnpr/v0/resolve`) remain bounded by `requestTimeout` (or a `10m` ceiling when `requestTimeout` is `0`) until resolution finishes, emitting a terminal `error` frame with code `ERR_PNPR_RESOLVE_TIMEOUT` if the deadline elapses.

Can be specified as an integer number of seconds (e.g. `30`) or with a duration suffix (e.g. `30s`, `1m`).

```yaml title="pnpr.yaml"
requestTimeout: 60s
```

## `shutdownTimeout`

How long pnpr waits for in-flight requests to finish after receiving `SIGINT` or `SIGTERM` before exiting.

Defaults to `25s`, leaving headroom inside Kubernetes' default 30-second `terminationGracePeriodSeconds` so `SIGKILL` does not land mid-drain. Must be greater than zero.

Can be specified as an integer number of seconds (e.g. `25`) or with a duration suffix (e.g. `25s`, `1m`).

```yaml title="pnpr.yaml"
shutdownTimeout: 25s
```

## `packumentTtl`

How long cached upstream packuments remain fresh before pnpr revalidates them against the upstream registry.

Defaults to `5m`. Can be overridden per-process with the `--packument-ttl` CLI flag.

Can be specified as an integer number of seconds (e.g. `10`) or with a duration suffix (e.g. `10s`, `1m`).

```yaml title="pnpr.yaml"
packumentTtl: 30s
```

## `maxTarballSize`

The largest tarball pnpr fetches from an upstream registry. The same limit
applies to Cargo crates and Python distributions fetched from an upstream. pnpr writes each
proxied tarball to a temporary file while it verifies the integrity, so this
bounds the disk each tarball can take. An upstream tarball that declares a size
over the limit is refused with `502 Bad Gateway`. pnpm then fails the install
with `ERR_PNPM_TARBALL_HTTP_STATUS`. When a cached upstream does not declare
the size, pnpr is already streaming the tarball when it crosses the limit, so
pnpr cuts the download off and the client reports a download error instead.
pnpr verifies the tarball before it responds for an upstream with
`cache: false` and for a [registry revision](https://pnpm.io/registry-revisions).
There an over-limit tarball always gets `502 Bad Gateway`, whether the upstream
declared the size or pnpr found it during the download.

Defaults to `100MiB`. Takes an integer number of bytes or a size suffix such
as `MB`, `MiB`, `GB`, or `GiB`, like [`cache.maxSize`](#storage-and-cache).
Must be greater than zero. The limit applies to downloads: a tarball pnpr
cached under a higher limit is still served from the cache.

An npm publish request is at most 100 MiB of JSON, including metadata and
base64-encoded attachments. Base64 increases the archive size by about a third,
so an inline tarball must be smaller than 75 MiB.
[Staged archives](ecosystems.md#staged-archives) accept archives up to 100 MiB
without base64 encoding. This setting only affects tarballs proxied from
upstream registries.

```yaml title="pnpr.yaml"
maxTarballSize: 256MiB
```

## `packageDocuments`

How pnpr serves its own package document, which pnpm asks for from a registry
declared [`serverType: pnpr`](https://pnpm.io/registries#servertype). See
[pnpr's package document](endpoints.md#pnprs-package-document).

```yaml title="pnpr.yaml"
packageDocuments:
  splitThreshold: 64KiB
  publishTimeWindow: 30d
```

| Field | Meaning |
| --- | --- |
| `splitThreshold` | The size of a package's document with every manifest inline above which only the versions a resolution likely picks keep their manifests. pnpr decides on every read, so a document can go back below it, for example when a publish after a long gap drops the publish times that fall out of the window. Defaults to `64KiB`. |
| `publishTimeWindow` | How far back from a package's last publish its versions keep their publish times. Defaults to `30d`, which is also the shortest window pnpr accepts. |

## `releaseRecords`

How pnpr logs the [release records](endpoints.md#pnprs-package-document) of
the versions it hosts. Each
hosted namespace has a transparency log per ecosystem, in the
[C2SP tiled log](https://c2sp.org/tlog-tiles) format Go's checksum database
uses. A version's log entry is recorded in the same transaction that accepts
the version. After the commit, pnpr adds the entry to the log's tiles and signs
a new checkpoint. Tiles and checkpoints are kept in the hosted store, beside
the archives.

```yaml title="pnpr.yaml"
releaseRecords:
  logSigningKey: ${PNPR_LOG_SIGNING_KEY}
  statementSigningKey: ${PNPR_STATEMENT_SIGNING_KEY}
```

| Field | Meaning |
| --- | --- |
| `logSigningKey` | The signed-note private key that signs every log's checkpoints, `PRIVATE+KEY+<name>+<key hash>+<key>`, as Go's [`note.GenerateKey`](https://pkg.go.dev/golang.org/x/mod/sumdb/note#GenerateKey) writes one. Its name starts every log's origin: `<name>/npm` for the default namespace, and `<name>/npm/~<namespace>` for another one. Each log's checkpoints are signed with the same Ed25519 key under the log's origin. Without a key, pnpr records log entries but publishes no logs. Use [environment variable substitution](#environment-variable-substitution) rather than writing the key into the file. |
| `statementSigningKey` | The signed-note private key, in the same format, that signs the registry's [in-toto statements](https://github.com/in-toto/attestation) about its records in DSSE envelopes. Its `<name>+<key hash>` is each signature's `keyid`. pnpr states that it accepted each logged record under its profile. Without a key, the records' evidence lists no statements. |

## `gc`

How [`pnpr gc`](cli.md#gc) reclaims archives that no file of a hosted package
maps. `lease` is how long an archive a reclaim moved to `reclaiming` and did
not finish removing, because the run or replica that started it stopped, is
left alone before another run removes it and records it gone. It only affects
how soon an interrupted reclaim is finished, never which archives are removed
or whether a publish is safe. Defaults to `1h` and must be positive. Takes an
integer number of seconds or a duration suffix, like the other intervals.

```yaml title="pnpr.yaml"
gc:
  lease: 1h
```

## Security limits and protections

pnpr includes built-in protections against denial-of-service and brute-force attacks:

- **Pre-buffer authorization**: Package publish and modification endpoints (`PUT /{pkg}`, `PUT /@{scope}/{pkg}`, `PUT /{pkg}/-rev/{rev}`) check caller authentication and write permissions before reading or buffering request payloads (up to 100 MiB). Unauthorized or unauthenticated requests are rejected immediately with `401 Unauthorized` or `403 Forbidden` without consuming server memory or network bandwidth.
- **Login rate limiting**: Login attempts (`PUT /-/user/org.couchdb.user:...`) are rate-limited to mitigate password brute-force and CPU exhaustion from bcrypt password verification. Failed login attempts are throttled by client IP and by username across all client IPs with a `429 Too Many Requests` response and a `Retry-After` header. The account limit is higher than the IP limit, so one IP cannot exhaust it.
- **Concurrency limits**: Intensive operations are governed by concurrency semaphores to protect against resource exhaustion (limiting concurrent password verifications and concurrent large payload buffering).
- **Upstream request coalescing (singleflight)**: Concurrent requests for the same uncached upstream packument are coalesced so multiple concurrent requests result in only a single upstream fetch.


## `ui`

Whether and from where pnpr serves its web UI. See [Web UI](web-ui.md#settings).

```yaml title="pnpr.yaml"
ui:
  enabled: true
```

## `auth`

Accounts and tokens live in the [database](storage.md#database),
next to package metadata: `<storage>/.metadata/metadata.sqlite` unless
`storage.database.url` names another one. Back up that database to keep them.

```yaml title="pnpr.yaml"
auth:
  users:
    # Self-registration is disabled when omitted or set to -1.
    # Set a non-negative cap to allow new users.
    maxUsers: -1
```

Omitted `maxUsers` means registration disabled, not unlimited registration.
Existing users can still log in. To bring accounts over from Verdaccio or
Apache, use [`pnpr import-htpasswd`](cli.md#import-htpasswd).

Two settings limit how long a token from a password login or
`npm token create` stays valid:

- `auth.tokens.defaultLifetime`: the lifetime of a login token, and of a
  created token that sets no expiry. Defaults to `90d`.
- `auth.tokens.maxLifetime`: the longest a token stays valid. A token with a
  later expiry expires after the maximum instead. Without the setting, a
  created token can set any expiry.

Both apply when pnpr issues a token. A token keeps the expiry it was issued
with, so changing either setting does not affect existing tokens. Each must be
at least `1m`. Units are `s`, `m`, `h`, `d`, and `w`. A
[browser sign-in](oidc.md) keeps its session's expiry of at most an hour.

```yaml title="pnpr.yaml"
auth:
  tokens:
    defaultLifetime: 30d
    maxLifetime: 52w
```

To share auth state across several stateless pnpr replicas, move users and
tokens into a shared SQL database. See [Shared database](auth-backends.md).

`auth.admins` lists the account IDs that may administer the registry. An admin
may:

- create and remove accounts, change passwords, and revoke tokens through the
  [admin endpoints](endpoints.md#admin-endpoints), even while self-registration
  is disabled;
- edit [teams](#managing-teams-through-the-api) and change
  [package access lists](#changing-rules-through-the-api).

Create the first admin with [`pnpr add-user`](cli.md#add-user), then copy the
printed account ID into `auth.admins`. Account IDs also belong in access lists
and team membership lists. Names are used for login and display. See
[Account and team identity](identity.md).

```yaml title="pnpr.yaml"
auth:
  admins: [<account-id>]
```

`auth.registryAdmins` lists the account IDs that may change the
[stored registries](#registries-in-the-database): add, replace, and remove
registries, and restore earlier revisions. A registry
admin may also edit teams and change package access lists, but does not
manage accounts unless `auth.admins` lists them too.

An `auth.oidc` list configures OpenID Connect browser sign-in and keyless CI
publishing alongside the password backend. See
[OpenID Connect](oidc.md).

`auth.scim` lets an identity provider deprovision accounts. See
[SCIM deprovisioning](scim.md).

## `secret`

```yaml title="pnpr.yaml"
# One secret for every purpose:
secret: ${PNPR_SECRET}

# Or one secret per purpose, so each rotates alone:
#secret:
#  cache: ${PNPR_CACHE_SECRET}
#  oci: ${PNPR_OCI_SECRET}
```

pnpr derives a key for each purpose from its secret:

- **`cache`**: private cache namespaces. Private resolution-cache entries and
  private per-registry cache directories are keyed with it, so on-disk paths
  reveal neither registry names nor credentials.
- **`oci`**: bearer token signing key for the OCI registry surface.

Each secret must be at least 16 bytes. Quote a secret that YAML would read as a
number, such as `secret: "${PNPR_SECRET}"` for an all-digit value. Changing the `cache` secret orphans the
private cache entries keyed with the old one. `pnpr cache gc` removes them.

When no `cache` secret is configured, pnpr generates a random one for a single
run on an empty local cache directory. pnpr refuses to start without one when
the cache directory already contains data from a previous run, is shared with
another process, or uses S3, libSQL, PostgreSQL, or MySQL backends. Without an
`oci` secret, OCI bearer tokens last only until pnpr restarts.

## Registry, resolver, artifact, and pipeline surfaces {#registry-resolver-and-artifact-surfaces}

pnpr exposes four independently deployable HTTP surfaces:

- The **registry surface**: package and tarball reads, publish,
  unpublish, dist-tags, and search, on each ecosystem's base and on every
  `/~<name>/` endpoint under it, for [every ecosystem](ecosystems.md). It has
  no config toggle. The CLI flag `--disable-registry` turns it off for one
  process.
- The **resolver surface**: the pnpr install-accelerator routes
  (`GET /-/pnpr`, `POST /-/pnpr/v0/resolve`, and
  `POST /-/pnpr/v0/verify-lockfile`). It is enabled by default; the CLI flag
  `--disable-resolver` overrides the setting:

```yaml title="pnpr.yaml"
resolver:
  enabled: true
```

- The **shared-artifact surface**: the three organization-scoped endpoints of
  the [shared side-effects cache](shared-side-effects-cache.md) proof of
  concept. It is off by default and is a top-level peer of `resolver`, so an
  I/O-bound artifact tier can scale independently of a compute-bound resolver:

```yaml title="pnpr.yaml"
artifacts:
  enabled: true
  owners:
    acme:
      access: [<workload-account-id>, <account-id-1>, <account-id-2>]
      publish: <workload-account-id>
```

  The CLI flag `--disable-artifacts` overrides this setting. With artifacts off,
  its routes are not mounted and the `GET /-/pnpr` handshake advertises an empty
  `protocols.artifacts.versions` list. The handshake itself is served when any surface is
  enabled, so an artifact-only tier is discoverable.

  `owners` declares the artifact owners this server keeps build output for, and who
  may read and who may publish what each one owns. An owner has three
  things:

  - its signed artifacts: [shared side effects](shared-side-effects-cache.md)
    and `pnpm pipeline` task results,
  - its [Cargo compilation cache](compiler-cache.md),
  - its [pipeline run records](pipeline-runs.md).

  `access` and `publish` list pnpr account IDs, or `$authenticated` for every
  signed-in account. `owners` cannot name a team, because teams belong to the
  [stored registries](#registries-in-the-database).
  Publishing also requires `access`. An empty list denies everyone. An owner nobody may read is invisible: requests for it answer
  as if it did not exist. An undeclared owner is unavailable.
  See [Access evaluation](access.md) for how these rules combine with authentication.
  `owners` is read even when `enabled` is false, for a server that only stores
  pipeline runs.

  `quota` bounds what the artifact store keeps: `perOwner` for one
  owner (an organization, or a cache name), defaulting to `1GiB`, and `total` for
  all of them together, defaulting to `10GiB`. Both must be positive, and `perOwner`
  cannot exceed `total`. A `pnpm pipeline` task result can hold
  release binaries, so a server that stores them usually needs more:

```yaml title="pnpr.yaml"
artifacts:
  quota:
    perOwner: 50GiB
    total: 200GiB
```

- The **pipeline run surface**: the endpoints that store and serve
  [`pnpm pipeline` run records](pipeline-runs.md), a peer of the artifact store.
  It is off by default:

```yaml title="pnpr.yaml"
pipeline:
  enabled: true
```

  Who may read and submit runs is decided by [`artifacts.owners`](#registry-resolver-and-artifact-surfaces).

Something must be served: a server with the registry surface disabled by flag,
the resolver disabled, artifacts disabled, and the pipeline surface disabled
is a startup error.

The health endpoint (`/-/ping`) and the account endpoints (login, whoami, and
token management) are always served, whichever surfaces are enabled, so
`pnpm login` works even against a resolver-only or artifact-only server. See
[HTTP endpoints](endpoints.md).

## `routes`

Resolver route classification (see
[Install acceleration](install-acceleration.md#private-registries)) treats the
official npm registry and `npm.jsr.io` as public out of the box. If clients resolve against
other registries that serve anonymously-readable content, declare them as
public routes so their resolutions can be cached and shared across all callers:

```yaml title="registries.yaml"
routes:
  public:
    - registry: https://registry.mirror.example.com/
    - registry: https://npm.corp.example.com/
      package: '@oss/*'
```

`routes.public` belongs to the
[registries document](#registries-in-the-database), not to the config file.
Each rule may name a `registry` origin, a `package` glob, or both. An
unparsable `registry` URL or `package` glob is refused.

The same public routes are the download allowlist of the Cargo and Python
proxies (see [Ecosystems](ecosystems.md)).

The resolver refuses to connect to loopback, private, and link-local
addresses, other than the `--public-url` origin. Another port of that host is
refused, even when the host is a public IP. A `routes.public` registry that the
resolver would refuse is refused when it is stored. To resolve through an upstream
on an internal network, list its addresses in `allowedPrivateNetworks`, which
stays in the config file, as IPs or CIDR blocks:

```yaml title="pnpr.yaml"
routes:
  allowedPrivateNetworks:
    - 10.0.0.0/8
```

## `osv`

Local OSV checks can hide or reject known vulnerable npm versions without live
OSV API calls:

```yaml title="pnpr.yaml"
osv:
  enabled: true
  path: ./osv/npm/all.zip
```

`path` may point to an OSV npm database zip or an extracted JSON directory. When
omitted, pnpr looks for `<storage>/.osv/npm/all.zip`. The same feature can be
enabled from the CLI with `--osv` and `--osv-db`.

## `log`

```yaml title="pnpr.yaml"
log:
  format: pretty   # or `json`
  level: error     # trace, debug, http, info, warn, or error
```

pnpr logs to stdout. `format` defaults to `pretty` and `level` to `info`.
`RUST_LOG` always overrides the configured level. See the
[CLI reference](cli.md#logging).

### Access logs and request IDs

Every request emits an access log record under the tracing target `pnpr::access` at `info` level:

- `request_id`: incoming `x-request-id` header (if valid printable ASCII, <=128 characters) or a generated RFC 4122 UUID v4. Also echoed in the response `x-request-id` header.
- `method`: HTTP method.
- `uri`: request path and query string.
- `client_ip`: client IP resolved according to [`trustedProxies`](#trustedproxies).
- `user`: authenticated username, or `"-"` when unauthenticated.
- `user_agent`: `User-Agent` header value, or `"-"` when omitted.
- `status`: HTTP response status code.
- `latency_ms`: request round-trip processing duration in milliseconds.
- `bytes`: response body size in bytes, derived from `Content-Length` (0 for chunked or streamed responses without length headers).

### Admin audit logs

Sign-ins, token changes, publishes, and administrative and destructive operations emit structured audit records under the tracing target `pnpr::audit` at `info` level. When structured logging is enabled (`format: json`), these events output as JSON objects that inherit the active request span context, including `request_id`, `client_ip`, `method`, and `uri`. The `pnpr::audit=info` directive is included across all configured log levels so audit events are retained by default. Operators can filter or silence them via `RUST_LOG` (for example, `RUST_LOG=pnpr::audit=off`).

Every audit event records:

- `action`: what happened, from the list below.
- `actor_id`: the account ID of whoever performed the action, or `"-"` for SCIM actions, unauthenticated callers, and pnpr itself. An account keeps its ID when it is renamed, so attribute actions by `actor_id`.
- `actor`: the account's name at the time of the action, `"scim"` for SCIM actions, or `"-"`.

Events about another account name it with `user` and `user_id`. The operation-specific attributes are:

- **Sign-in and tokens**:
  - `auth.sign_in`: signing in with a password through `npm login`, or through an OIDC provider (`method` is `password` or `oidc`, `provider` for OIDC).
  - `auth.login_failed`: a refused password sign-in, or a wrong password when a signed-in account confirms it (`user`, `method`).
  - `auth.workload_use`: a request authenticated with OIDC workload credentials (`subject`, `registry`). The actor is the account the workload is bound to.
  - `token.create`: creating a token (`token_key`, `readonly`).
  - `token.revoke`: revoking one of the caller's own tokens, by key or on logout (`token_key`).
- **User management**:
  - `user.create`: creating an account (`actor`, `user`, `user_id`).
  - `user.password_update`: changing an account password (`actor`, `user`, `user_id`).
  - `user.rename`: renaming an account (`actor`, `user`, `user_id`).
  - `user.token_revoke`: revoking user tokens (`actor`, `user`, `user_id`, `token_key`).
  - `user.delete`: deleting an account (`actor`, `user`, `user_id`, `tokens_revoked`).
  - `user.register`: self-registering a new account (`actor`, `user`, `user_id`).
- **Registries stored in the database**:
  - `registries.replace`: replacing every stored registry at once (`actor`, `version`, `previous_version`).
  - `registries.update`: adding or replacing one registry (`actor`, `registry`, `version`, `previous_version`).
  - `registries.delete`: removing one registry (`actor`, `registry`, `version`, `previous_version`).
  - `registries.restore`: restoring an earlier revision (`actor`, `restored`, `version`, `previous_version`).
  - `registries.import`: storing registries with `pnpr registries import` (`actor`, `version`, `previous_version`). The actor is `-`.
  - `registries.seed`: storing the default registries on the first start against an empty database (`actor`, `version`). The actor is `-`.
  - `rules.replace`: replacing a registry's package access lists (`actor`, `registry`, `ecosystem`, `version`).
- **Team administration and package access**:
  - `team.create`: creating a team (`actor`, `scope`, `team`).
  - `team.destroy`: deleting a team (`actor`, `scope`, `team`).
  - `team.member_add`: adding a user to a team (`actor`, `scope`, `team`, `user`, `user_id`).
  - `team.member_remove`: removing a user from a team (`actor`, `scope`, `team`, `user`, `user_id`).
  - `team.access_grant`: granting package permissions to a team (`actor`, `scope`, `team`, `package`, `permissions`).
  - `team.access_revoke`: revoking package permissions from a team (`actor`, `scope`, `team`, `package`).
- **SCIM provisioning**:
  - `scim.user_create`: provisioning a SCIM user (`actor`, `user`, `user_id`, `active`).
  - `scim.user_replace`: replacing SCIM user attributes (`actor`, `user`, `user_id`, `active`).
  - `scim.user_patch`: updating SCIM user status (`actor`, `user`, `user_id`, `active`).
  - `scim.user_delete`: deprovisioning a SCIM user (`actor`, `user`, `user_id`).
- **Publishing, unpublishing, and package mutations**:
  - `package.publish`: committing a publish, once per package (`actor`, `package`, `ecosystem`, `releases`, `namespace`). `releases` lists what the publish adds, comma-separated: npm and Cargo versions, Python distribution filenames, or OCI manifest digests. A retried request that pnpr answers from its recorded outcome is not audited again.
  - `package.unpublish`: unpublishing an entire package (`actor`, `package`, `registry`).
  - `package.unpublish_tarball`: unpublishing a specific package version tarball (`actor`, `package`, `tarball`, `version`, `registry`).
  - `package.update_packument`: modifying packument metadata directly (`actor`, `package`, `registry`).
  - `package.update_maintainers`: changing a package's maintainers (`actor`, `package`, `registry`).
  - `package.add_owners` and `package.remove_owners`: changing a package's owners (`actor`, `package`, `registry`, `owners`). `owners` lists the logins added or removed, comma-separated.
  - `package.dist_tag_set`: assigning a dist-tag (`actor`, `package`, `tag`, `version`, `registry`).
  - `package.dist_tag_remove`: deleting a dist-tag (`actor`, `package`, `tag`, `registry`).
  - `package.note_set` and `package.note_delete`: changing a version note (`actor`, `package`, `version`, `registry`).
  - `publication.note_change`: changing a publication note (`actor`, `namespace`, `publication`).
  - `oci.delete_blob`: deleting an OCI blob (`actor`, `repository`, `digest`).
  - `oci.delete_manifest`: deleting an OCI manifest (`actor`, `repository`, `reference`).

- **Archive collection**:
  - `gc.remove_archive`: reclaiming an unmapped archive (`actor`, `key`, `size`). The collection span names the `namespace` and `ecosystem`. A removal after unmapping an unreachable image blob also names the `repository`.

### Prometheus metrics

pnpr serves Prometheus metrics in the standard text format (`text/plain; version=0.0.4`) only when the config has a `metrics` block. The block needs a `token`, a `listen` address, or both:

```yaml title="pnpr.yaml"
metrics:
  token: ${PNPR_METRICS_TOKEN}
  listen: 127.0.0.1:9464
```

- `token`: the bearer token a scraper sends, at least 32 characters. Without `listen`, pnpr serves metrics at `GET /-/metrics` and `GET /metrics` on its main address, and refuses a request without the token with `401`.
- `listen`: an address for a second listener that serves only `GET /metrics`. The main address then serves no metrics. Expose the main address and keep this one on a private network. With `token` set, the second listener requires it too.

A Prometheus scrape job sends the token with `authorization`:

```yaml title="prometheus.yml"
scrape_configs:
  - job_name: pnpr
    authorization:
      credentials_file: /etc/prometheus/pnpr-metrics-token
    static_configs:
      - targets: ["pnpr.internal:9464"]
```

The metrics are:

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
storage:
  s3:
    bucket: my-pnpr-packages
    accessKeyId: ${PNPR_S3_ACCESS_KEY_ID}
    secretAccessKey: ${PNPR_S3_SECRET_ACCESS_KEY}
```

`${ENV_VAR-default}` supplies `default` when `ENV_VAR` is unset, and
`${ENV_VAR:-default}` also when it is empty. An unset or empty variable without
a default becomes an empty string, and pnpr logs a warning.

An unset variable in a field that holds a credential fails startup, so pnpr
never runs with an empty credential. These fields are `secret`, `secret.cache`,
`secret.oci`, `storage.database.url`, `storage.database.authToken`, `storage.s3.secretAccessKey`,
`auth.scim.token`, OIDC `clientSecret`, `metrics.token`,
`cloudflareCdn.apiToken`, and the `releaseRecords` signing keys.

The [stored registries](#registries-in-the-database) are not part of the
file. They take `${ENV_VAR}` references only in upstream credentials. See
[Upstream credentials](#upstream-credentials).

## cloudflareCdn

Optional global CDN invalidation for a registry served through a Cloudflare zone.
`zoneId` must contain 32 hexadecimal characters. `apiToken` must be nonempty
and contain no whitespace. `cacheTag` accepts 1 to 100 ASCII letters, digits,
hyphens, or underscores. The API token needs
Cache Purge permission for that zone. Use environment substitution for the token.

See [CDN cache with push invalidation](cloudflare.md#cdn-cache-with-push-invalidation)
for the matching Worker, retry behavior, and access-revocation delay.
