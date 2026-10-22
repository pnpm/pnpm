---
id: auth-backends
title: Auth backends (shared SQL)
---

Auth state (the registered users and their bearer tokens) is per-instance disk
state. By default users live in an htpasswd file and tokens in a local SQLite
database (see [`auth`](configuration.md#auth)), so two pnpr replicas don't see
each other's accounts.

Adding a `backend:` block moves both into one shared SQL database, so several
stateless replicas share a consistent set of logins and tokens. This is the auth half
of running pnpr horizontally scaled. Only one backend may be selected in a
config file.

Database drivers are Cargo-feature gated:

| Backend | Config key | Cargo feature |
| --- | --- | --- |
| libsql / Turso | `backend.libsql` | `backend-libsql` (enabled by default) |
| PostgreSQL | `backend.postgres` or `backend.postgresql` | `backend-postgres` |
| MySQL-compatible | `backend.mysql` | `backend-mysql` |

For PostgreSQL or MySQL support, build pnpr with the matching Cargo feature, for
example `cargo build -p pnpr --features backend-postgres`.

:::tip

Token lookups happen on the request hot path, so the database should be
low-latency from the server.

:::

## libsql / Turso

```yaml title="pnpr.yaml"
storage: ./storage

backend:
  libsql:
    # libsql/Turso database URL. `libsql://…` for Turso, or
    # `http://127.0.0.1:8080` for a local `sqld`.
    url: ${PNPR_LIBSQL_URL}
    # Bearer token for the database. Omit for an unauthenticated local `sqld`.
    authToken: ${PNPR_LIBSQL_TOKEN}
```

| Key | Required | Description |
| --- | --- | --- |
| `url` | yes | Database URL: `libsql://<db>.turso.io` (Turso) or `http://<host>:<port>` (self-hosted `sqld`). |
| `authToken` | no | Bearer token for the database. Omit for an unauthenticated local `sqld`. pnpr sends it only over `https://` or `libsql://`, or over `http://` to a loopback host, and refuses to start otherwise. |
| `replicaPath` | no | Path to a local **embedded replica**. When set, reads (token lookups) hit this local file instead of a network round-trip; writes still go to the primary. Absent ⇒ every read is a remote query. |
| `syncInterval` | no | How often the embedded replica pulls from the primary. Accepts an interval string (such as `60s` or `1m`) or integer seconds. Only meaningful with `replicaPath`; bounds how stale a read can be (token-revocation lag). `0` disables background sync. Defaults to `60s`. `syncIntervalSecs` is also accepted. |
| `maxConnections` | no | Most auth database operations pnpr runs at once, each on a connection of its own. Defaults to `10`. |

Token lookups happen on the request hot path, so for a remote primary such
as Turso Cloud or a self-hosted `sqld`, set `replicaPath` to serve reads from
a local replica. pnpr uses the sync protocol the primary offers, so the same
setting works with Turso Cloud and with a self-hosted `sqld`.

```yaml title="pnpr.yaml"
backend:
  libsql:
    url: ${PNPR_LIBSQL_URL}
    authToken: ${PNPR_LIBSQL_TOKEN}
    replicaPath: ./auth-replica.db
    syncInterval: 60s
```

The trade-off is read freshness: an embedded replica reflects another replica's
writes (a token issued or revoked elsewhere) only after the next background
sync, so a lower positive `syncInterval` means less revocation lag. Omit
`replicaPath` to always read the primary directly.

## PostgreSQL

```yaml title="pnpr.yaml"
backend:
  postgres:
    url: ${PNPR_POSTGRES_URL}
    maxConnections: 16
    timeout: 30s
    startupTimeout: 5m
```

## MySQL

```yaml title="pnpr.yaml"
backend:
  mysql:
    url: ${PNPR_MYSQL_URL}
    maxConnections: 16
    timeout: 30s
    startupTimeout: 5m
```

| Key | Required | Description |
| --- | --- | --- |
| `url` | yes | Driver connection URL, such as `postgres://user:pass@host/db` or `mysql://user:pass@host/db`. |
| `maxConnections` | no | Maximum connections in the backend pool. Omit to use the driver's default. |
| `timeout` | no | Request-path auth database deadline. Defaults to `30s`. |
| `startupTimeout` | no | Startup connection and schema setup deadline. Defaults to `5m`. |

When the `backend:` block is absent, the `auth.htpasswd` / `auth.tokens`
settings apply. Without `auth.htpasswd.file`, accounts
[live in memory](configuration.md#auth), and so do tokens unless
`auth.tokens.file` is set. With a `backend:` block,
`auth.htpasswd.file` and `auth.tokens.file` are ignored, and only the
`auth.htpasswd.max_users` registration cap is read. Omitted
`max_users` and `max_users: -1` both disable self-registration; set a
non-negative cap to let new users register.

## Extensibility model

Custom user and token backends are compiled into pnpr. There is no plugin
runtime.
