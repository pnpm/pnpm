---
id: auth-backends
title: Shared database
---

Auth state (the registered users and their bearer tokens) is per-instance disk
state. By default users live in a JSON account file and tokens in a local SQLite
database (see [`auth`](configuration.md#auth)), so two pnpr replicas don't see
each other's accounts.

Set `storage.database.url` to a libSQL, PostgreSQL, or MySQL database to share accounts,
tokens, and package metadata across instances. Archives stay in the configured
filesystem or S3 store. PostgreSQL and MySQL share one connection pool for auth
and metadata, limited by `storage.database.maxConnections`.

pnpr supports these databases:

| Backend | Config key | Status |
| --- | --- | --- |
| libsql / Turso | `storage.database.url: libsql://…` | Stable |
| PostgreSQL | `storage.database.url: postgres://…` | Stable |
| MySQL 8.0.19 or later | `storage.database.url: mysql://…` | Experimental |

:::tip

Token lookups happen on the request hot path, so the database should be
low-latency from the server.

:::

## libsql / Turso

```yaml title="pnpr.yaml"
storage:
  path: ./storage
  database:
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
| `syncInterval` | no | How often the embedded replica pulls from the primary. Accepts an interval string (such as `60s` or `1m`) or integer seconds. Only meaningful with `replicaPath`; bounds how stale a read can be (token-revocation lag). `syncInterval` must be positive when `replicaPath` is set. Defaults to `60s`. |
| `maxConnections` | no | Most database operations pnpr runs at once, each on a connection of its own. Defaults to `10`. |

Token lookups happen on the request hot path, so for a remote primary such
as Turso Cloud or a self-hosted `sqld`, set `replicaPath` to serve reads from
a local replica. pnpr uses the sync protocol the primary offers, so the same
setting works with Turso Cloud and with a self-hosted `sqld`.

```yaml title="pnpr.yaml"
storage:
  database:
    url: ${PNPR_LIBSQL_URL}
    authToken: ${PNPR_LIBSQL_TOKEN}
    replicaPath: ./auth-replica.db
    syncInterval: 60s
```

The trade-off is read freshness: an embedded replica reflects another replica's
writes (a token issued or revoked elsewhere) only after the next background
sync, so a lower positive `syncInterval` means less revocation lag. Omit
`replicaPath` to always read the primary directly.

The metadata replica uses `replicaPath`. The auth replica uses the same path
with `.auth` appended. Back up the primary database; these local files can be
recreated from it.

## PostgreSQL

```yaml title="pnpr.yaml"
storage:
  database:
    url: ${PNPR_POSTGRES_URL}
    maxConnections: 16
    timeout: 30s
    startupTimeout: 5m
```

pnpr uses the connection's current schema. Instances on different schemas of
one database keep separate data and do not wait on each other's locks. Set the
schema with the `options` URL parameter, for example
`postgres://user:pass@host/db?options=-c%20search_path%3Dpnpr`. The schema
must exist before pnpr starts, and the database user needs permission to create
tables in it.

## MySQL

:::warning

MySQL support is experimental, and pnpr logs a warning at startup when it is
configured. MySQL commits each schema change on its own, so a migration step
that fails partway stays half applied until pnpr starts again and reruns it.
Use SQLite, libSQL, or PostgreSQL for production data.

:::

```yaml title="pnpr.yaml"
storage:
  database:
    url: ${PNPR_MYSQL_URL}
    maxConnections: 16
    timeout: 30s
    startupTimeout: 5m
```

MySQL metadata keys are case-sensitive and limited to 512 bytes each. This
includes namespace, package, version, profile, tag, artifact, and publication
names, principal IDs, and idempotency keys. Longer keys are rejected
without committing the publication. Package manifests, READMEs, release records,
and other payloads are not subject to this key limit. Use MySQL 8.0.19 or later
with InnoDB and its default 16 KiB page size.

| Key | Required | Description |
| --- | --- | --- |
| `url` | yes | Driver connection URL, such as `postgres://user:pass@host/db` or `mysql://user:pass@host/db`. |
| `maxConnections` | no | Maximum connections in the backend pool. Omit to use the driver's default. |
| `timeout` | no | Database operation deadline. Defaults to `30s`. |
| `startupTimeout` | no | Initial connection and schema operation deadline. Defaults to `5m`. |

Accounts and tokens live in the same database as package metadata. When
`storage.database.url` is absent or uses `sqlite:`, that is the SQLite metadata
database. The `auth.users.maxUsers` registration cap applies to every
database. Omitted `maxUsers` and `maxUsers: -1` both disable
self-registration; set a non-negative cap to let new users register.

Passwords must contain at least 8 characters and at most 71 bytes.
This applies to registration, login, administrator-created accounts, and password changes.

Account creation and renaming reject usernames that mix Unicode scripts, contain
restricted Unicode characters, or share a case- and accent-folded Unicode
confusable skeleton with another account. Recreate pre-1.0 SQL auth databases
that lack a `schema_migrations` table; databases at schema version `1` or later
migrate automatically at startup (see
[Schema migrations](storage.md#metadata-schema-changes)).

## Extensibility model

Custom user and token backends are compiled into pnpr. There is no plugin
runtime.

## Deleted accounts

Deleting an account retains its immutable ID under a generated deleted name,
revokes its credentials, and frees its login name. A new account using that
name receives a fresh ID. Team memberships and package ownership remain bound
to the deleted ID. Deleted accounts are hidden from user lists and do not count
toward `maxUsers`. See [Account and team identity](identity.md).

## Changing passwords

Use `npm profile set password` to change your password. Accounts provisioned
without a password cannot use this command to enroll one. Changing a password,
including an administrator reset, revokes the account's existing tokens.
Sign in again after the change.
