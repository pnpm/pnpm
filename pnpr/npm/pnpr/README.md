# @pnpm/pnpr

A pnpm-compatible npm registry server, written in Rust. Speaks the npm
registry protocol, so any npm-compatible client (pnpm, npm, yarn) can
talk to it. Proxies packages from a configured upstream like
npmjs.org and serves them with its own auth and access controls.

Lives in the [pnpm monorepo](https://github.com/pnpm/pnpm) under
[`pnpr/`](https://github.com/pnpm/pnpm/tree/main/pnpr).

## Install

```sh
pnpm add -g @pnpm/pnpr
```

The wrapper resolves to the native binary published under
`@pnpm/pnpr.<platform>-<arch>` (e.g. `@pnpm/pnpr.linux-x64`). Prebuilt
binaries are available for `linux-x64`, `linux-arm64`, `linux-x64-musl`,
`linux-arm64-musl`, `darwin-x64`, `darwin-arm64`, `win32-x64`, and
`win32-arm64`.

## Usage

Start the server with the bundled default config:

```sh
pnpr
```

It listens on `127.0.0.1:7677` and proxies `https://registry.npmjs.org/`
by default. Point a client at it with:

```sh
pnpm config set registry http://127.0.0.1:7677/
```

## CLI flags

| Flag | Description |
| --- | --- |
| `-c, --config <path>` | Path to a verdaccio-shaped YAML config. When omitted, the bundled default is used. |
| `--listen <addr>` | Address to bind to. Defaults to `127.0.0.1:7677`. |
| `--storage <path>` | Override the storage directory from the loaded config. |
| `--cache <path>` | Override the disposable proxy-cache directory (the mirror of upstream registries plus the resolver cache). Defaults to a `.pnpr-cache` subdirectory of `--storage`. |
| `--public-url <url>` | URL clients should use to reach the server, used when rewriting `dist.tarball` in served packuments. Defaults to `http://<listen>`. |
| `--packument-ttl-secs <n>` | Seconds before a cached packument is considered stale and refetched. |

Log level is controlled via the standard `RUST_LOG` environment
variable (e.g. `RUST_LOG=debug pnpr`).

## Configuration

`pnpr` uses a [verdaccio](https://verdaccio.org/docs/configuration)-shaped
YAML config. A minimal example:

```yaml
storage: ./storage

uplinks:
  npmjs:
    url: https://registry.npmjs.org/

packages:
  '@*/*':
    access: $all
    publish: $authenticated
    unpublish: $authenticated
    proxy: npmjs

  '**':
    access: $all
    publish: $authenticated
    unpublish: $authenticated
    proxy: npmjs
```

Pass it with `-c`:

```sh
pnpr -c ./pnpr.yaml
```

### Storing hosted packages in S3 / Cloudflare R2

`pnpr` keeps two kinds of data:

- **Hosted** — the source of truth: packages published to this server
  plus anything served in static mode. This lives under `storage`.
- **Cache** — the disposable mirror of upstream registries plus the
  resolver cache. This lives under `cache` (defaults to
  `<storage>/.pnpr-cache`).

By default both are local directories. Adding an `s3:` block moves the
**hosted** store into an S3-compatible object store, so the durable data
is replicated by the provider and can be shared by several stateless
`pnpr` replicas. The cache stays on local disk — only the hosted
package store is pluggable here.

Because any S3-compatible endpoint works, this also covers **Cloudflare
R2**, **MinIO**, **Backblaze B2**, **Wasabi**, etc. — point `endpoint`
at the right host.

```yaml
storage: ./storage   # still backs the local cache + upload staging

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

| Key | Required | Description |
| --- | --- | --- |
| `bucket` | yes | Bucket the hosted packages are stored in. |
| `region` | no | AWS S3 needs a real region (e.g. `us-east-1`); Cloudflare R2 uses `auto`. |
| `endpoint` | no | Custom endpoint for S3-compatible providers. Omit for AWS S3; for R2 it's `https://<account-id>.r2.cloudflarestorage.com`; for MinIO it's e.g. `http://127.0.0.1:9000`. |
| `prefix` | no | Key prefix every object is stored under. |
| `accessKeyId` | no | Access key. Falls back to `AWS_ACCESS_KEY_ID` when unset. |
| `secretAccessKey` | no | Secret key. Falls back to `AWS_SECRET_ACCESS_KEY` when unset. |
| `forcePathStyle` | no | Use path-style addressing (`endpoint/bucket/key`) instead of virtual-hosted (`bucket.endpoint/key`). MinIO typically needs `true`; AWS and R2 work with the default. |
| `allowHttp` | no | Allow plain-HTTP endpoints — needed for a local MinIO over `http://`. Defaults to HTTPS-only. |

Any `${ENV_VAR}` in the config is substituted from the environment
before parsing, so secrets can be kept out of the file. Keeping the
credentials in `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` and
omitting them from the YAML works too.

Run it the same way as any other config:

```sh
AWS_ACCESS_KEY_ID=… AWS_SECRET_ACCESS_KEY=… pnpr -c ./pnpr.yaml
```

A complete R2 example, end to end:

```yaml
# pnpr.yaml
storage: ./storage

s3:
  bucket: my-pnpr-packages
  region: auto
  endpoint: https://abc123def456.r2.cloudflarestorage.com

uplinks:
  npmjs:
    url: https://registry.npmjs.org/

packages:
  '**':
    access: $all
    publish: $authenticated
    unpublish: $authenticated
    proxy: npmjs
```

```sh
export AWS_ACCESS_KEY_ID="<r2-access-key-id>"
export AWS_SECRET_ACCESS_KEY="<r2-secret-access-key>"
pnpr -c ./pnpr.yaml --listen 0.0.0.0:7677 --public-url https://registry.example.com
```

(`--public-url` is what rewrites the `dist.tarball` URLs in served
packuments, so clients fetch tarballs back through this server rather
than the upstream.)

A local MinIO over plain HTTP needs `forcePathStyle` and `allowHttp`:

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

### Storing users and tokens in a shared SQL database

Auth state — the registered users and their bearer tokens — is the other
piece of per-instance disk state. By default users live in an
htpasswd file and tokens in a local SQLite database (see `auth:` above),
so two `pnpr` replicas don't see each other's accounts. Adding a
`backend:` block moves both into one shared SQL database, so several
stateless replicas share a consistent set of logins and tokens — the
auth half of running `pnpr` horizontally scaled.

The same auth traits drive every backend, and the SQL schema sticks to
common column types so records can be moved between supported drivers.
Only one backend may be selected in a config file.

Database drivers are Cargo-feature gated:

| Backend | Config key | Cargo feature |
| --- | --- | --- |
| libsql / Turso | `backend.libsql` | `backend-libsql` (enabled by default) |
| PostgreSQL | `backend.postgres` or `backend.postgresql` | `backend-postgres` |
| MySQL-compatible | `backend.mysql` | `backend-mysql` |

Token lookups happen on the request hot path, so the database should be
low-latency from the server.

```yaml
storage: ./storage

backend:
  libsql:
    # libsql/Turso database URL. `libsql://…` for Turso, or
    # `http://127.0.0.1:8080` for a local `sqld`.
    url: ${PNPR_LIBSQL_URL}
    # Bearer token for the database. Omit for an unauthenticated local
    # `sqld`.
    authToken: ${PNPR_LIBSQL_TOKEN}
```

| Key | Required | Description |
| --- | --- | --- |
| `url` | yes | Database URL — `libsql://<db>.turso.io` (Turso) or `http://<host>:<port>` (self-hosted `sqld`). |
| `authToken` | no | Bearer token for the database. Omit for an unauthenticated local `sqld`. |
| `replicaPath` | no | Path to a local **embedded replica**. When set, reads (token lookups) hit this local file instead of a network round-trip; writes still go to the primary. Absent ⇒ every read is a remote query. |
| `syncIntervalSecs` | no | How often (seconds) the embedded replica pulls from the primary. Only meaningful with `replicaPath`; bounds how stale a read can be (token-revocation lag). `0` disables background sync. Defaults to `60`. |

Token lookups happen on the request hot path, so for a remote primary
(e.g. Turso) set `replicaPath` to serve reads from a local replica:

```yaml
backend:
  libsql:
    url: ${PNPR_LIBSQL_URL}
    authToken: ${PNPR_LIBSQL_TOKEN}
    replicaPath: ./auth-replica.db
    syncIntervalSecs: 60
```

The trade-off is read freshness: an embedded replica reflects another
replica's writes (a token issued or revoked elsewhere) only after the
next background sync, so lower `syncIntervalSecs` means less
revocation lag. Omit `replicaPath` to always read the primary directly.

PostgreSQL:

```yaml
backend:
  postgres:
    url: ${PNPR_POSTGRES_URL}
    maxConnections: 16
```

MySQL:

```yaml
backend:
  mysql:
    url: ${PNPR_MYSQL_URL}
    maxConnections: 16
```

For PostgreSQL or MySQL support, build pnpr with the matching Cargo
feature, for example `cargo build -p pnpr --features backend-postgres`.

When the `backend:` block is absent, auth stays on local disk and the
`auth.htpasswd` / `auth.tokens` settings apply as before. The
`auth.htpasswd.max_users` registration cap is honored either way.

## License

Source-available under the [PolyForm Shield License 1.0.0](https://github.com/pnpm/pnpm/blob/main/pnpr/LICENSE.md) — **not** open source. You may run, modify, and self-host pnpr for any purpose except providing a product that competes with it. Commercial / non-compete licenses are available from Zoltan Kochan (<https://kochan.io>).

## Trademark notice

pnpr is not affiliated with, endorsed by, or sponsored by npm, Inc., GitHub, or Microsoft. "npm" is a trademark of npm, Inc., used here only to describe compatibility with the npm registry protocol.
