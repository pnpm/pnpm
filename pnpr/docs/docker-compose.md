---
id: docker-compose
title: Docker Compose and Coolify
---

pnpr ships a Compose file that runs the
[container image](installation.md#container-image) with durable storage:
[`pnpr/examples/docker-compose`](https://github.com/pnpm/pnpm/tree/main/pnpr/examples/docker-compose).
It holds two files:

- `compose.yaml` runs `ghcr.io/pnpm/pnpr` with a volume for storage and one for
  the cache, and publishes the port on loopback.
- `pnpr.yaml` is the server config, mounted at `/pnpr/config.yaml`. It sets the
  [`secret`](configuration.md#secret), which a cache that outlives the container
  requires, and the log format.

## Run it

Image tags are pnpr versions, such as the one
`npm view @pnpm/pnpr dist-tags.next` prints. Until the first stable release
there is no `latest` tag, so the Compose file needs an exact version. Generate
the secret once and keep it: changing it orphans the private cache entries
keyed with the old one. The example's `.gitignore` keeps `.env`, which holds
the secret, out of version control.

```sh
cd pnpr/examples/docker-compose
cat > .env <<EOF
PNPR_IMAGE_TAG=<version>
PNPR_SECRET=$(openssl rand -hex 32)
PNPR_PUBLIC_URL=http://localhost:7677
EOF
docker compose up -d
docker compose exec pnpr pnpr add-user alice
```

The server starts with the
[default registries](configuration.md#registries-in-the-database): `/npm/`
proxies the public npm registry, and `/npm/~local/` hosts private packages and
takes publishes. Self-registration is disabled, so create accounts with
[`pnpr add-user`](cli.md#add-user). Point the public registry and your scope
at them, then sign in and publish:

```sh
pnpm config set registry http://localhost:7677/npm/
pnpm config set @acme:registry http://localhost:7677/npm/~local/
pnpm login --registry http://localhost:7677/npm/~local/
pnpm publish
```

To serve both from one URL, import a router as shown in the
[Quick start](quick-start.md) with
`docker compose exec pnpr pnpr registries import <file>`.

| Variable | Default | Purpose |
| --- | --- | --- |
| `PNPR_IMAGE_TAG` | none, required | The `ghcr.io/pnpm/pnpr` tag to run. |
| `PNPR_SECRET` | none, required | The [`secret`](configuration.md#secret), at least 16 bytes. |
| `PNPR_PUBLIC_URL` | `http://localhost:7677` | The URL clients reach pnpr at. pnpr rewrites `dist.tarball` URLs onto it, so set it to the public HTTPS URL in production. |
| `PNPR_CACHE_MAX_SIZE` | `20GB` | The largest the proxy cache grows before pnpr evicts the least recently used entries. Anonymous reads of the public npm registry fill it. |
| `PNPR_LOG_FORMAT` | `json` | `json` or `pretty`. |
| `PNPR_LOG_LEVEL` | `info` | `trace`, `debug`, `http`, `info`, `warn`, or `error`. |

pnpr reads every other [CLI option](cli.md#environment-variables) from a
`PNPR_*` variable, so add those to the `environment` block. Settings without a
flag go in `pnpr.yaml`.

The `pnpr-storage` volume holds published packages and the database with
accounts and tokens. Back it up as described in
[Backup and restore](backup-and-restore.md). The `pnpr-cache` volume is
disposable, and `PNPR_CACHE_MAX_SIZE` bounds it.

## Coolify

[Coolify](https://coolify.io) deploys the Compose file from a Git repository
and puts its Traefik proxy, with automatic HTTPS, in front of it.

1. Copy `compose.yaml` and `pnpr.yaml` into a repository Coolify can reach.
2. In your Coolify project, add a resource from that repository and choose the
   **Docker Compose** build pack. Point it at `compose.yaml`.
3. Remove the `ports` entry. Coolify's proxy reaches the container over its own
   network, and a published port would bypass the proxy.
4. Assign a domain to the `pnpr` service with port `7677`, such as
   `https://registry.example.com:7677`. Coolify routes the domain to that
   container port and provisions the certificate.
5. Set the environment variables: `PNPR_IMAGE_TAG`, `PNPR_SECRET`, and
   `PNPR_PUBLIC_URL` set to the domain from the previous step without the
   port, such as `https://registry.example.com`.
6. Deploy, then create the first account from the container's terminal in
   Coolify: `pnpr add-user alice`.
7. Point clients at `https://registry.example.com/npm/` and your scope at
   `https://registry.example.com/npm/~local/`, as in [Run it](#run-it).

Coolify keeps the named volumes across redeploys. Without them, every
redeploy loses published packages, accounts, and tokens.

pnpr sees Coolify's proxy as the client of every request. To log, rate-limit,
and restrict tokens by the real client address, add the proxy's network to
[`trustedProxies`](tls-and-reverse-proxy.md#trustedproxies) in `pnpr.yaml`.
