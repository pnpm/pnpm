---
id: cli
title: CLI reference
---

`pnpr` runs the registry server when no subcommand is given. The other
subcommands are maintenance tools.

```sh
pnpr [serve] [OPTIONS]
pnpr config check
pnpr verify [OPTIONS]
pnpr export --out <dir> [OPTIONS]
pnpr migrate-storage [OPTIONS]
pnpr import-htpasswd --source <path> [OPTIONS]
pnpr add-user <username>
pnpr gc [OPTIONS]
pnpr cache-gc [OPTIONS]
```

## Options

Server options apply to `pnpr serve` (and bare `pnpr`). Global path options (`-c`, `--storage`, `--cache`) apply across subcommands.

| Flag | Description |
| --- | --- |
| `-c, --config <path>` | Path to a [YAML config](configuration.md). When omitted, the global `config.yaml` in pnpr's config dir is used if it exists, otherwise the bundled default config. |
| `--listen <addr>` | Address to bind to. Defaults to `127.0.0.1:7677`. |
| `--storage <path>` | Override the [storage](configuration.md) directory from the loaded config. This directory is the source of truth for hosted packages. A [metadata database](storage.md#sqlite-metadata) at the default path, `<storage>/.metadata/metadata.sqlite`, moves with it; one anywhere else stays where it is. |
| `--cache <path>` | Override the disposable cache directory (the mirror of upstream registries, resolver cache, and local shared-artifact store). Defaults to a `.pnpr-cache` subdirectory of the storage path. |
| `--public-url <url>` | URL clients should use to reach the server, used when rewriting `dist.tarball` URLs in served packuments. Defaults to `http://<listen>`. |
| `--packument-ttl <interval>` | Duration before a cached packument is considered stale and refetched (e.g. `5m`, `300s`, or bare seconds). Defaults to [`packumentTtl`](configuration.md#packumentttl), which defaults to `5m`. A registry's `maxAge` overrides it for that registry. |
| `--osv [<bool>]` | Enable local [OSV](https://osv.dev/) npm vulnerability checks. Requires a local OSV npm database zip at `--osv-db` or `<cache>/osv/npm/all.zip`. `--osv false` turns off checks that the config enables. |
| `--osv-db <path>` | Path to the local OSV npm database zip or extracted JSON directory. |
| `--disable-registry` | Disable the npm registry surface (packument and tarball reads, publish, unpublish, dist-tags, and search) that is otherwise served whenever the config declares at least one registry. The health and login/token endpoints stay available. |
| `--disable-resolver` | Disable the install-accelerator surface: `GET /-/pnpr`, `POST /-/pnpr/v0/resolve`, and `POST /-/pnpr/v0/verify-lockfile`. Overrides `resolver.enabled`. |
| `--disable-artifacts` | Disable the signed shared-artifact surface. Overrides `artifacts.enabled` independently of the resolver. |
| `--disable-pipeline` | Disable the pipeline run-record surface: `/-/pnpr/v0/pipeline`. Overrides `pipeline.enabled`. |
| `-h, --help` | Print help. |
| `-V, --version` | Print version. |

## Environment variables

pnpr reads every option in the [Options](#options) table, except `--help` and
`--version`, from an environment variable when the flag is omitted. The variable is the flag's name with a
`PNPR_` prefix, so `--config` becomes `PNPR_CONFIG`, `--storage` becomes
`PNPR_STORAGE`, `--cache` becomes `PNPR_CACHE`, `--public-url` becomes
`PNPR_PUBLIC_URL`, and `--disable-resolver` becomes `PNPR_DISABLE_RESOLVER`.
The flags of the maintenance subcommands have no environment variable. A flag
given on the command line wins over its variable. Boolean flags accept `true`, `1`, `yes`, `on`, `false`, `0`, `no`,
and `off`.

```sh
PNPR_LISTEN=0.0.0.0:7677 PNPR_PUBLIC_URL=https://registry.example.com pnpr
```

## Subcommands

### config check

Validates the YAML configuration file and prints diagnostic details, including configured registries, upstreams, hosted storage, cache directory, auth backends, and enabled surface toggles (`registry`, `resolver`, `artifacts`).

```sh
pnpr config check
```

### verify

Audits hosted package documents (`package.json`) against stored blobs. Inspects declared versions, resolves referenced tarballs and attestation blobs, checks for missing or corrupted files, and discovers orphaned blobs not referenced by any version.

```sh
# Full checksum verification (SHA-1 and Subresource Integrity)
pnpr verify

# Check that every blob exists, without comparing hashes
pnpr verify --skip-hashes
```

`verify` reports orphaned blobs and changes nothing; [`gc`](#gc) removes them.

| Flag | Description |
| --- | --- |
| `--skip-hashes` | Skip recomputing and verifying tarball checksums (cryptographic hashes are verified by default). |

### export

Exports hosted packages, packuments, and tarball blobs to a local directory.

```sh
pnpr export --out /mnt/backups/pnpr-export --include-auth
```

| Flag | Description |
| --- | --- |
| `--out <dir>` | Destination directory for exported packages and files. Required. |
| `--include-auth` | Include configured auth files (`htpasswd`, `tokens.db`) in the output directory (excluded by default). Files are created with restricted (`0600`) permissions. |

### migrate-storage

Migrates hosted package blobs from local filesystem storage to an S3-compatible object store. Package metadata stays in its [database](storage.md#libsql-metadata), which an S3 deployment reads through a libSQL `metadata.url`. Move the metadata into that database first, for example by importing the SQLite file into Turso, and point `metadata.url` at it. A run that is not `--dry-run` refuses any other metadata.

```sh
# Preview packages and blobs that would be transferred
pnpr migrate-storage --to-bucket my-bucket --dry-run

# Transfer and verify blob content on the destination
pnpr migrate-storage --to-bucket my-bucket --verify
```

| Flag | Description |
| --- | --- |
| `--from <dir>` | Source hosted storage directory. Defaults to the configured [`storage`](configuration.md#storage-and-cache) directory (`--storage`). The packages come from the configured metadata database; this directory supplies their blobs. |
| `--to-bucket <name>` | Destination S3 bucket name. |
| `--to-region <region>` | Destination S3 AWS region (e.g. `us-east-1`). |
| `--to-endpoint <url>` | Custom S3 endpoint URL (for MinIO, Cloudflare R2, etc.). |
| `--to-prefix <prefix>` | Destination key prefix within the bucket. |
| `--dry-run` | Report packages and blobs to migrate without uploading. |
| `--verify` | Read back and verify content of uploaded blobs after transfer. |

Without `--to-bucket`, the command migrates into the bucket of the config's
[`s3:`](storage.md#backends) block. With `--to-bucket`, only the `--to-*` flags
describe the destination. The region, endpoint, and prefix are not read from
the config, credentials come only from `AWS_ACCESS_KEY_ID` and
`AWS_SECRET_ACCESS_KEY`, and `forcePathStyle` and `allowHttp` cannot be set.
To reach a plain-HTTP MinIO, configure the `s3:` block instead:

```sh
pnpr -c ./pnpr-s3.yaml migrate-storage --from ./storage --verify
```

### import-htpasswd

Imports users from an Apache or Verdaccio `htpasswd` file into pnpr's user store. Supports standard bcrypt hashes (`$2a$`, `$2b$`, `$2y$`) as well as legacy `{SHA}` hashes (which are upgraded to bcrypt upon first login).

Run this command as an offline maintenance step while pnpr is stopped, or specify an unshared `--target` path.

```sh
# Preview users that would be imported
pnpr import-htpasswd --source /path/to/old/htpasswd --dry-run

# Import and merge with pnpr's configured htpasswd file
pnpr import-htpasswd --source /path/to/old/htpasswd
```

| Flag | Description |
| --- | --- |
| `--source <path>` | Path to the source htpasswd file to import. Required. |
| `--target <path>` | Path to destination htpasswd file. Defaults to `auth.htpasswd.file` from config. |
| `--dry-run` | Preview imported user counts without writing to target file. |
| `--overwrite` | Overwrite conflicting credentials for existing users in destination file (skipped by default). |

### add-user

Creates a user account, or changes the password of an existing one. Use it to
create admin accounts, which cannot self-register. Creating an account revokes
any tokens left under that username.

The command needs `auth.htpasswd.file` or a database backend. With the
htpasswd file, run it while pnpr is stopped. A running pnpr does not see the
new account, and its next account change overwrites the file.

```sh
# Prompts for the password
pnpr add-user alice

# Reads the password from one line of stdin
printf '%s\n' "$PASSWORD" | pnpr add-user alice
```

The password is read from the terminal, or from one line of stdin when stdin is
not a terminal. It cannot be passed as a flag.

### gc

Removes the archives no file of a hosted package maps, in every hosted
registry: npm, PyPI, Cargo, and OCI. It is safe to run while the registry
serves and other replicas publish.

```sh
# Report what would go
pnpr gc --dry-run

# Collect one registry
pnpr gc --registry local
```

A namespace stores each archive's bytes once, under an incarnation of their
identity, and its metadata maps every published file to one (see
[Layout](storage.md#layout)). `gc` removes:

- the bytes nothing maps any more: those of unpublished versions and packages
  whose removal did not finish reclaiming them, and image blobs no retained
  manifest reaches;
- incarnations no commit made live: what a publish that stopped before its
  commit, or one refused at it, stored;
- reclaims a run or replica started and did not finish, once the
  [`gc.lease`](configuration.md#gc) has passed since they started.

An OCI blob stays while a retained manifest reaches it: untagged manifests,
the children of retained indexes, and their config and layer blobs. A
repository whose retained manifest is missing or corrupt keeps every blob, and
`gc` reports the error after collecting the rest. A name in the store that is
no key pnpr stores archives under is never removed.

The metadata database fences every removal; no step depends on timing. A
reclaim first moves the bytes' live incarnation to `reclaiming`, in a
transaction that holds only while no file maps them, then removes exactly that
incarnation's key, then records it gone. A publish that depends on that
incarnation is refused with `409` and can be retried: the retry stores the
bytes again as the next incarnation, under another key, so a removal that
lands late never removes them. An incarnation a publish on its way may still
commit is first recorded gone, which refuses that commit, and only once it was
stored longer than `--min-age` ago.

| Flag | Description |
| --- | --- |
| `--registry <name>` | A hosted registry to collect, by its config name. Repeat it for more. Defaults to every hosted registry. |
| `--dry-run` | Report the archives that would be removed without removing them. The report is a snapshot taken without the fence, so it can differ from a real run: it also counts incarnations another run is still removing, which a real run leaves to that run until `gc.lease` has passed, and publishes landing meanwhile can keep archives it lists. |
| `--min-age <interval>` | Keep an incarnation no commit made live, and an image blob no manifest reaches yet, until it is older than this (e.g. `1d`, `86400s`, or bare seconds). Defaults to `1d`, so a publish or image push still in flight is not refused. `0` collects them all. |

### cache-gc

Added in: v0.1.0-alpha.16

Reclaims cached upstream tarballs and packuments, prunes orphaned namespaces
left behind by per-process secrets or changed configurations, and enforces
cache size and age limits.

| Flag | Description |
| --- | --- |
| `--dry-run` | Report the candidates without deleting anything. |
| `--max-size <size>` | Maximum total cache size to retain (e.g. `10GB`, `500MB`, or raw bytes). Evicts oldest files first (LRU). Overrides the config value. |
| `--max-age <interval>` | Maximum file age (e.g. `30d`, `1h`, or bare seconds). Entries older than this are deleted. Overrides the config value. |
| `--orphans-only` | Only prune orphaned upstream namespaces without evicting active cache entries. |

## Logging

The log level is controlled with the standard `RUST_LOG` environment variable,
which always wins over the level set in the config file:

```sh
RUST_LOG=debug pnpr
```

The config `log.level` accepts `trace`, `debug`, `http`, `info`, `warn`, and
`error`. `info` (the default) and `http` both log one access record per request,
and `warn` and `error` drop them. To keep access records while quieting the
rest, set
`RUST_LOG=warn,pnpr::access=info,pnpr::audit=info`. A `RUST_LOG` value that
omits `pnpr::audit=info` drops the audit records below `warn`.

## Examples

Bind to all interfaces and tell clients to reach the server through a public
hostname:

```sh
pnpr --listen 0.0.0.0:7677 --public-url https://registry.example.com
```

Run with a custom config and a separate, ephemeral cache volume:

```sh
pnpr -c ./pnpr.yaml --cache /mnt/ephemeral/pnpr-cache
```

Run only the pnpr resolver endpoints, without exposing an npm registry surface:

```sh
pnpr --disable-registry
```

## Signals and graceful shutdown

pnpr handles `SIGINT` (Ctrl-C) and Unix `SIGTERM`:

1. **Ceases new connections**: The server stops accepting new incoming TCP connections on its listen address.
2. **Drains in-flight requests**: Active requests and ongoing package transfers are allowed to complete cleanly before the process exits.

Configuration changes are applied via process restart rather than in-process reloading (such as SIGHUP). In container orchestrators (such as Kubernetes or Docker Swarm), use rolling deployments so the new replica's readiness probe (`GET /-/ready`) passes before the old replica receives `SIGTERM` and drains.

## Exit codes

pnpr uses standard exit codes:

| Code | Meaning | Description |
| --- | --- | --- |
| `0` | Success | The command completed successfully. For the server, indicates clean termination after `SIGINT` or `SIGTERM`. For a maintenance subcommand, indicates all checks and transfers succeeded. |
| `1` | Error | The command failed. Causes include invalid CLI flags, YAML configuration errors, address binding conflicts (port already in use), missing storage/auth files, or integrity audit failures (such as `pnpr verify` finding missing or corrupted blobs). |

In Unix shells, if an external supervisor kills the process before the graceful shutdown handler finishes, the exit status will reflect the signal number (for example, `130` for `SIGINT` or `143` for `SIGTERM`).

