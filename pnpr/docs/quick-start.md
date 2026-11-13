---
id: quick-start
title: Quick start
---

## Write a config

pnpr routes packages through declared
[registries](configuration.md#registries-and-defaultregistry): every origin
(a locally-hosted registry, an upstream like npmjs) is declared explicitly and
claims the package names it serves, and a router resolves each name to exactly
one of them. The registries live in pnpr's database, apart from the server
config. Save the server config as `pnpr.yaml`:

```yaml title="pnpr.yaml"
storage: ./storage
secret: ${PNPR_SECRET}

auth:
  users:
    # Allow one user to self-register, for the publish step below.
    maxUsers: 1
```

Save the registries as `registries.yaml`:

```yaml title="registries.yaml"
registries:
  # Packages published to this server. The `packages:` map claims the
  # package names served here: only these names can live here.
  local:
    type: hosted
    packages:
      '@mycompany/*':
        publish: $authenticated

  # The public npm registry. No `packages:` map — it serves every name,
  # so it must be the last source in the router below.
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true

  # A package resolves to the first source that claims its name:
  # your scope goes to the local registry, everything else to npm.
  main:
    type: router
    sources: [local, npmjs]

# The bare server URL serves the router.
defaultRegistry: main
```

Replace `@mycompany` with your own scope. There is no fall-through between
registries: a `@mycompany/*` package that isn't published locally is a
definitive 404, never a request to npmjs. This is what closes
dependency confusion.

## Start the server

Generate a secret once, store it securely, and reuse the same `PNPR_SECRET`
value across restarts. Store the registries with
[`pnpr registries import`](cli.md#registries), then start the server:

```sh
export PNPR_SECRET="$(openssl rand -hex 32)"
pnpr -c ./pnpr.yaml registries import ./registries.yaml
pnpr -c ./pnpr.yaml
```

It listens on `127.0.0.1:7677`. Running plain `pnpr` with no config works for a
single ephemeral run too, using a random cache secret until `.pnpr-cache` is
removed or `secret` is configured in `pnpr.yaml`. Without an import, the first
start stores the
[default registries](configuration.md#registries-in-the-database): a proxy of
npmjs and a local hosted registry at `/npm/~local/`, whose packages need a
login. The bundled config allows no self-registration, so create accounts with
[`pnpr add-user`](cli.md#add-user).

## Point a client at it

Configure pnpm (or npm/yarn) to use the local server as its registry:

```sh
pnpm config set registry http://127.0.0.1:7677/npm/
```

Now `pnpm install` fetches packages through pnpr. Packages proxied from the
upstream are cached, so subsequent installs are served locally.

Each registry is also directly addressable at
`http://127.0.0.1:7677/npm/~<name>/`. For example, you can point only your scope
at the hosted registry instead of using the router.

## Publish a package

Create a user and publish:

```sh
pnpm login --registry http://127.0.0.1:7677/npm/
pnpm publish --registry http://127.0.0.1:7677/npm/
```

The publish routes through the router to the `local` hosted registry.
Publishing a package that resolves to an upstream, or whose name no registry
claims, is rejected.

## Where to go next

- [CLI reference](cli.md): every flag `pnpr` accepts.
- [Configuration](configuration.md): write your own config, including registries,
  access rules, and auth.
- [Reverse proxy and TLS](tls-and-reverse-proxy.md): set up Nginx, Caddy, Traefik, or a cloud load balancer for production.
- [Webhooks and event notifications](webhooks.md): stream audit events and storage notifications to external webhooks.
- [Backup and restore](backup-and-restore.md): snapshot and restore package storage and user records.
- [Install acceleration](install-acceleration.md): let pnpr resolve your
  dependency graph to speed up installs.

