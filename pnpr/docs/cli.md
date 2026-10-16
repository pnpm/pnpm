---
id: cli
title: CLI reference
---

```sh
pnpr [OPTIONS]
pnpr config check
pnpr verify [OPTIONS]
pnpr export --out <dir> [OPTIONS]
pnpr migrate-storage [OPTIONS]
pnpr import-htpasswd --source <path> [OPTIONS]
pnpr [OPTIONS] oci-gc --registry <name>
pnpr [OPTIONS] cache-gc [CACHE_OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `-c, --config <path>` | Path to a [YAML config](configuration.md). When omitted, the global `config.yaml` in pnpr's config dir is used if it exists, otherwise the bundled default config. |
| `--listen <addr>` | Address to bind to. Defaults to `127.0.0.1:7677`. |
| `--storage <path>` | Override the [storage](configuration.md) directory from the loaded config — the source of truth for hosted packages. |
| `--cache <path>` | Override the disposable cache directory (the mirror of upstream registries, resolver cache, and local shared-artifact store). Defaults to a `.pnpr-cache` subdirectory of the storage path. |
| `--public-url <url>` | URL clients should use to reach the server, used when rewriting `dist.tarball` URLs in served packuments. Defaults to `http://<listen>`. |
| `--packument-ttl-secs <n>` | Seconds before a cached packument is considered stale and refetched. Defaults to `300`. A registry's `maxage` overrides it for that registry. |
| `--osv` | Enable local [OSV](https://osv.dev/) npm vulnerability checks. Requires a local OSV npm database zip at `--osv-db` or `<cache>/osv/npm/all.zip`. |
| `--osv-db <path>` | Path to the local OSV npm database zip or extracted JSON directory. |
| `--disable-registry` | Disable the npm registry surface (packument and tarball reads, publish, unpublish, dist-tags, and search) that is otherwise served whenever the config declares at least one registry. The health and login/token endpoints stay available. |
| `--disable-resolver` | Disable the install-accelerator surface: `GET /-/pnpr`, `POST /-/pnpr/v0/resolve`, and `POST /-/pnpr/v0/verify-lockfile`. Overrides `resolver.enabled`. |
| `--disable-artifacts` | Disable the signed shared-artifact surface. Overrides `artifacts.enabled` independently of the resolver. |
| `-h, --help` | Print help. |
| `-V, --version` | Print version. |

## Every flag has an environment variable

Since v0.1.0-alpha.11, pnpr reads any option from an environment variable when
the flag is omitted. The variable is the flag's name with a `PNPR_` prefix, so
`--public-url` becomes `PNPR_PUBLIC_URL` and `--disable-resolver` becomes
`PNPR_DISABLE_RESOLVER`. A flag given on the command line wins over its
variable. Boolean flags accept `true`, `1`, `yes`, `on`, `false`, `0`, `no`,
and `off`.

```sh
PNPR_LISTEN=0.0.0.0:7677 PNPR_PUBLIC_URL=https://registry.example.com pnpr
```

## Subcommands

### config check

Validates the YAML configuration file and prints diagnostic details, including configured registries, upstreams, hosted storage, cache directory, auth backends, and enabled surface toggles (`registry`, `resolver`, `artifacts`).

```sh
pnpr config check
# or using the hyphenated alias
pnpr config-check
```

### verify

Audits hosted package documents (`package.json`) against stored blobs. Inspects declared versions, resolves referenced tarballs and attestation blobs, checks for missing or corrupted files, and discovers orphaned blobs not referenced by any version.

```sh
# Fast presence check
pnpr verify

# Full checksum verification (SHA-1 and Subresource Integrity)
pnpr verify --check-hashes

# Prune unreferenced blobs left behind by interrupted operations
pnpr verify --prune-orphans
```

| Flag | Description |
| --- | --- |
| `--check-hashes` | Recompute and verify tarball SHA-1 (`shasum`) and sha512 (`integrity`) checksums. |
| `--prune-orphans` | Automatically delete orphaned blobs not referenced by any hosted package version. |

### export

Exports hosted packages, packuments, and tarball blobs to a local directory.

```sh
pnpr export --out /mnt/backups/pnpr-export --include-auth
```

| Flag | Description |
| --- | --- |
| `--out <dir>` | Destination directory for exported packages and files. Required. |
| `--include-auth` | Also copy configured `htpasswd` and `tokens.db` files into the output directory. |

### migrate-storage

Migrates hosted packages and blobs from local filesystem storage to an S3-compatible object store.

```sh
# Preview packages and blobs that would be transferred
pnpr migrate-storage --to-bucket my-bucket --dry-run

# Transfer and verify blob sizes on the destination
pnpr migrate-storage --to-bucket my-bucket --verify
```

| Flag | Description |
| --- | --- |
| `--from <dir>` | Source hosted storage directory. Defaults to configured `storage.hosted_dir`. |
| `--to-bucket <name>` | Destination S3 bucket name. |
| `--to-region <region>` | Destination S3 AWS region (e.g. `us-east-1`). |
| `--to-endpoint <url>` | Custom S3 endpoint URL (for MinIO, Cloudflare R2, etc.). |
| `--to-prefix <prefix>` | Destination key prefix within the bucket. |
| `--dry-run` | Report packages and blobs to migrate without uploading. |
| `--verify` | Read back and verify size of uploaded blobs after transfer. |

### import-htpasswd

Imports users from an Apache or Verdaccio `htpasswd` file into pnpr's user store. Supports standard bcrypt hashes (`$2a$`, `$2b$`, `$2y$`) as well as legacy `{SHA}` hashes (which are upgraded to bcrypt upon first login).

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

### oci-gc

Added in: v0.1.0-alpha.11

Reclaims [container image](container-images.md#reclaiming-space) blobs that no
retained manifest references. Every writer sharing the store must be stopped for
the whole run.

| Flag | Description |
| --- | --- |
| `--registry <name>` | The concrete hosted OCI registry to collect, by its config name. Required. |
| `--dry-run` | Report the candidates without deleting anything. |
| `--min-age-secs <n>` | Keep blobs younger than this. Defaults to `86400`, so an interrupted push can still resume. `0` collects every unreferenced blob. |

### cache-gc

Added in: v0.1.0-alpha.16

Reclaims cached upstream tarballs and packuments, prunes orphaned namespaces
left behind by per-process secrets or changed configurations, and enforces
cache size and age limits.

| Flag | Description |
| --- | --- |
| `--dry-run` | Report the candidates without deleting anything. |
| `--max-size <size>` | Maximum total cache size to retain (e.g. `10GB`, `500MB`, or raw bytes). Evicts oldest files first (LRU). Overrides the config value. |
| `--max-age-secs <n>` | Maximum file age in seconds. Entries older than this are deleted. Overrides the config value. |
| `--orphans-only` | Only prune orphaned upstream namespaces without evicting active cache entries. |

## Logging

The log level is controlled with the standard `RUST_LOG` environment variable,
which always wins over the level set in the config file:

```sh
RUST_LOG=debug pnpr
```

The config `log.level` accepts `trace`, `debug`, `http`, `info`, `warn`, and
`error`. `http` enables one access-log record per request while keeping the
rest of the crate quieter.

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

pnpr handles `SIGINT` (Ctrl-C) and Unix `SIGTERM`. When either signal is received, the server initiates a graceful shutdown, stopping new connections and completing in-flight requests. This ensures clean termination in containerized environments such as Docker and Kubernetes.
