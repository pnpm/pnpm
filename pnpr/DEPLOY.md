# Deploying pnpr

`pnpr` is a single long-running HTTP server backed by a filesystem
store, so it deploys like any stateful web service: one instance, one
persistent volume, a domain with TLS in front.

> **Licensing.** `pnpr` is source-available under the
> [PolyForm Shield License](../LICENSE.md), **not** open source. You may
> run, modify, and self-host it for any purpose **except** providing a
> product that competes with `pnpr`. Self-hosting a private registry for
> your own team or CI is fine; offering it as a commercial hosted
> registry needs a commercial license from
> [Zoltan Kochan](https://kochan.io).

## Container image

The repo ships a multi-stage [`Dockerfile`](./Dockerfile). Its build
context is the **monorepo root**, because pnpr depends on `pacquet-*`
crates from the shared Cargo workspace. Build it from the repo root:

```bash
docker build -f pnpr/Dockerfile -t pnpr .
```

Run it with a persistent volume for state:

```bash
docker run -d --name pnpr \
  -p 4873:4873 \
  -v pnpr-data:/data \
  -e PNPR_PUBLIC_URL=https://registry.example.com \
  pnpr
```

Then point pnpm at it:

```bash
pnpm config set registry http://localhost:4873/
```

## Configuration

The image is configured through environment variables (translated into
CLI flags by [`docker/entrypoint.sh`](./docker/entrypoint.sh)):

| Variable | Default | Purpose |
| --- | --- | --- |
| `PNPR_PUBLIC_URL` | `http://<listen>` | URL clients use to reach the server. **Set this in production** — pnpr rewrites `dist.tarball` URLs in served packuments to it. Without it, clients get links pointing at the container's bind address. |
| `PNPR_LISTEN` | `0.0.0.0:4873` | Address the server binds to inside the container. |
| `PNPR_STORAGE` | `/data` | Directory for the package cache, `htpasswd`, and `tokens.db`. Mount a volume here. |
| `PNPR_CONFIG` | `/etc/pnpr/config.yaml` | Path to the YAML config. The baked-in [`docker/config.yaml`](./docker/config.yaml) proxies all of npm and persists state under `PNPR_STORAGE`. |
| `PNPR_PACKUMENT_TTL_SECS` | (config value) | Seconds before a cached packument is refetched. |
| `PNPR_LOG_LEVEL` / `PNPR_LOG_FORMAT` | `info` / `json` | Log verbosity and shape. `RUST_LOG` overrides the level. |

To customize package routing, auth, or uplinks beyond what the env vars
cover, mount your own YAML over `/etc/pnpr/config.yaml` (or point
`PNPR_CONFIG` elsewhere). It follows verdaccio's `config.yaml` shape;
`${VAR:-default}` placeholders are substituted from the environment.

## Coolify

[Coolify](https://coolify.io) is a self-hosted PaaS. pnpr fits its
Dockerfile/Compose deployment model directly.

1. **New Resource → Application** in your Coolify project, sourced from
   this Git repository.
2. **Build pack: Dockerfile.** Set:
   - **Base Directory:** `/` (the repo root — the build context needs the
     whole Cargo workspace).
   - **Dockerfile Location:** `pnpr/Dockerfile`.
3. **Port:** expose `4873` so Coolify's Traefik proxy routes to it.
4. **Domain:** assign one (e.g. `registry.example.com`). Coolify
   provisions HTTPS automatically.
5. **Environment variables:** set `PNPR_PUBLIC_URL` to that same HTTPS
   domain. Add `PNPR_LOG_LEVEL` / `PNPR_LOG_FORMAT` if you want to tune
   logging.
6. **Persistent storage:** add a volume mounted at `/data`. This is the
   one step that bites people — without it, every redeploy wipes your
   published packages, accounts, and cache.
7. Deploy. The image's `HEALTHCHECK` hits `/-/ping`, which Coolify
   surfaces as the container's health status.

Prefer Compose? Point Coolify at [`docker-compose.yml`](./docker-compose.yml)
instead — it already wires up the port, the `/data` volume, and the env
vars; just set `PNPR_PUBLIC_URL`.

## Operational notes

- **Single instance.** pnpr is a stateful single process over a
  filesystem store. Don't scale it to multiple replicas behind a load
  balancer expecting them to share state.
- **Back up `/data`.** It holds published packages, the htpasswd user
  file, and the SQLite token database.
- **Accounts.** `npm adduser --registry <url>` registers a user (written
  to `htpasswd`). Set `auth.htpasswd.max_users: -1` in the config once
  your accounts exist to close registration.
