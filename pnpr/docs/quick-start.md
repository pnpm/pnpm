---
id: quick-start
title: Quick start
---

## Write a config

pnpr routes packages through declared
[registries](configuration.md#registries-and-defaultregistry): every origin —
a locally-hosted registry, an upstream like npmjs — is declared explicitly and
claims the package names it serves, and a router resolves each name to exactly
one of them. Save this as `pnpr.yaml`:

```yaml
storage: ./storage

auth:
  htpasswd:
    file: ./htpasswd
    # Allow one user to self-register, for the publish step below.
    max_users: 1

registries:
  # Packages published to this server. The `packages:` map is this
  # registry's namespace: only these names can live here.
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
definitive 404, never a request to npmjs — which is exactly what closes
dependency confusion.

## Start the server

```sh
pnpr -c ./pnpr.yaml
```

It listens on `127.0.0.1:7677`. (Running plain `pnpr` with no config works
too, but the bundled default config is shaped for pnpm's own test registry —
for anything real, pass your own config.)

## Point a client at it

Configure pnpm (or npm/yarn) to use the local server as its registry:

```sh
pnpm config set registry http://127.0.0.1:7677/
```

Now `pnpm install` fetches packages through pnpr. Packages proxied from the
upstream are cached, so subsequent installs are served locally.

Each registry is also directly addressable at
`http://127.0.0.1:7677/~<name>/` — for example, you can point just your scope
at the hosted registry instead of using the router.

## Publish a package

Create a user and publish:

```sh
pnpm login --registry http://127.0.0.1:7677/
pnpm publish --registry http://127.0.0.1:7677/
```

The publish routes through the router to the `local` hosted registry —
publishing a package that resolves to an upstream, or whose name no registry
claims, is rejected.

## Where to go next

- [CLI reference](cli.md) — every flag `pnpr` accepts.
- [Configuration](configuration.md) — write your own config: registries,
  access rules, and auth.
- [Install acceleration](install-acceleration.md) — let pnpr resolve your
  dependency graph to speed up installs.
