---
id: install-acceleration
title: Install acceleration
---

Besides acting as a registry, pnpr can speed up `pnpm install` by resolving a
project's dependency graph **server-side** and handing pnpm a ready-to-use
lockfile. pnpm then runs a normal headless install, fetching every tarball
directly from the registries in parallel.

## Why it's faster

The slow part of installing a fresh project (one with no lockfile, or with new
dependencies) is **resolution**, not downloading tarballs. To resolve a
dependency graph, pnpm has to walk it: fetch a package's metadata (its
"packument"), read its dependency ranges, fetch the packuments for those, and so
on, level by level. Each level depends on the answers from the previous one, so
the requests can't all be parallelized. The total time is roughly the
**depth of the graph × the round-trip latency** to the registry. From a laptop
or a CI runner that sits far from the registry, that round-trip depth dominates
the install.

pnpr collapses that depth:

- **It runs the resolution close to the registry**, with low latency to the
  upstream, so each of those sequential round-trips is cheap.
- **It keeps a warm, shared cache** of packuments and verified resolution
  results across every client that uses it. A team or CI fleet pointed at one
  pnpr server resolves against a cache that's already hot from everyone else's
  installs.
- **It returns the whole resolved lockfile in a single response**, so the client
  pays one round-trip for resolution instead of one per graph level.

Crucially, pnpr only does the resolution. The tarballs are still fetched by the
client, **directly from the registries in parallel**, like a normal
install, which a single connection through the server could never match. The
accelerated path removes the latency-bound resolution waterfall without turning
tarball downloads into a single server-mediated stream.

When the project already has an up-to-date lockfile, there's nothing to resolve,
so this path doesn't apply. The speedup is for cold installs and dependency
changes.

## How it works

1. pnpm sends `POST /-/pnpr/v0/resolve` with the project's dependencies,
   registry settings, policy settings, and the existing lockfile when one is
   available. Client upstream credentials are never sent. Private registries
   are reached through the server's own credentials (see
   [below](#private-registries)). A lockfile-repair request first reads
   `GET /-/pnpr` to check that the server supports repair. An ordinary resolve
   is sent directly.
2. The server verifies the input lockfile under the client's policy, resolves
   against the client's registries, and streams an `application/x-ndjson`
   response. `package` frames announce resolved tarballs as the tree walk
   progresses, and a terminal `done` frame carries the resolved lockfile and
   stats. A terminal `violations` or `error` frame aborts the install.
3. pnpm writes the resolved lockfile and fetches tarballs directly from the
   registries, like a normal install.

For frozen restores with an already-fresh lockfile, pnpm can use
`POST /-/pnpr/v0/verify-lockfile` to get only the server-side trust verdict
instead of resolving again.

pnpr caches client configurations and evicts the least recently used entries
when the cache is full. Active requests keep their configuration until they
finish.

Up to 64 resolution and lockfile verification requests can be admitted at once
per server instance, and up to 8 per caller identity. Further requests return
`503 Service Unavailable` until admitted work finishes. Up to 16 admitted
requests run at once per server instance, and up to 4 per caller identity.
Queued requests wait for a slot up to the request deadline
(`504 Gateway Timeout`).

Oversized resolver settings return `400 Bad Request`. Lockfile verification
with `trustLockfile` skips configuration interning while still enforcing
admission, concurrency, deadline, request body, and dependency graph limits.

## Cargo and Python

Added in: v0.1.0-alpha.11

`POST /-/pnpr/v0/resolve` takes an `ecosystem` field, so the same endpoint
resolves [Cargo and Python](ecosystems.md) dependency graphs as well. Both are
latency-bound in the same way npm is, and each has its own extra cost the server
removes:

- For **Cargo**, the client no longer fetches one sparse-index file per crate in
  the graph.
- For **Python**, the client no longer downloads a wheel to find out what it
  requires.

A `cargo.enabled` or `python.enabled` workspace with `pnprServer` set offloads
those resolutions automatically. When the server does not serve an ecosystem's
resolution, pnpm resolves it locally instead of failing.

`pnpm install --fix-lockfile` is forwarded as a
repair request, including during filtered installs. The server regenerates
broken lockfile metadata while retaining locked versions that remain
compatible. The `fixLockfile` feature in `pnpr.protocols.resolver.features` tells the client
whether the advertised resolver versions support that request.

## What a configured server does not cost

An install skips the exchanges it doesn't need, so a configured `pnprServer`
does not make an up-to-date project pay a round trip that a direct install
would have answered locally:

- The repeat-install "Already up to date" fast path runs with a pnpr server
  configured. Changing the `trustPolicy*`, `minimumReleaseAgeStrict`, or
  `minimumReleaseAgeExclude` settings still invalidates it.
- An install whose `pnpm-lock.yaml` already satisfies every manifest skips the
  resolve exchange and materializes `node_modules` from the on-disk lockfile.
- The verification round trip is skipped when the local
  `lockfile-verified.jsonl` cache already covers the lockfile under the current
  policy. Server-verified and server-resolved lockfiles are recorded into that
  cache, so the next install can use it.

## Which registries the server will touch {#allowlist}

The resolver only fetches from origins the operator has configured: a
**default-deny allowlist** made of the stored
[registries](configuration.md#registries-and-defaultregistry), the declared
[public routes](configuration.md#routes), the built-in official npm registry
and `npm.jsr.io`, and pnpr's own origin. A request whose `registry`/`registries` (or whose
direct `http(s)`/`git` URL dependencies, overrides, or lockfile tarball URLs)
point anywhere else is rejected before any server-side fetch, and every
redirect hop is re-checked against the same allowlist. A caller can't use the
resolver to make the server fetch from an arbitrary internal address. An
allowlisted host that resolves to a loopback, private, or link-local address is
refused too, unless it is the `--public-url` origin or
[`routes.allowedPrivateNetworks`](configuration.md#routes) lists it.

## Private registries {#private-registries}

Clients never forward their upstream registry credentials to pnpr. Instead, a
private registry is configured server-side as an
[upstream registry](configuration.md#upstream-registries) with a server-owned
credential and an `access:` policy naming which pnpr users may resolve through
it. The caller authenticates to pnpr with their own pnpr token; pnpr selects
its own credential for each upstream fetch. One upstream token, held only by
the server, serves a whole team.

Tarball URLs in the resolved lockfile follow the same split:

- A **public** package keeps its upstream URL. The client fetches it directly
  from the registry or its CDN, never through pnpr.
- A **private** package resolved through an access-bearing upstream registry
  is emitted as that registry's `/~<name>/` endpoint URL, so the client
  fetches it back through pnpr (which enforces the registry's access policy
  and serves it from a per-registry private cache).

Because a registry endpoint URL is canonical for a client whose registry
configuration points at it, lockfile entries stay integrity-only (the
registry host lives in the client's `.npmrc`, not in the lockfile), and a
project resolves to the same lockfile whether it installs through `/resolve`
or directly against the registry endpoint.

When an npm registry URL points to this pnpr server's `public_url`, resolution
reads hosted packages and named upstreams such as `/npm/~internal/` inside the
server, using the caller's access permissions. Upstream credentials are sent
only to the configured upstream, never to `public_url` or its reverse proxy.

The resolution cache is **authorization-aware**: a resolution that touched
only public routes is cached once and shared by every caller, while a
resolution that touched private routes is keyed by the access it required and
is only replayed for callers who still hold that access at the current
upstream credential. Rotating an upstream credential automatically moves new
resolutions to a fresh cache namespace.

## Enabling it

Set `pnprServer` in your `pnpm-workspace.yaml` to the URL of a pnpr server:

```yaml title="pnpm-workspace.yaml"
pnprServer: http://127.0.0.1:7677
```

pnpm will offload resolution to that server during `pnpm install`. The resolver
`POST` endpoints require a valid pnpr `Authorization` header. pnpm forwards the
auth configured for the `pnprServer` URL, and a normal
`pnpm login --registry <server-url>` writes the token pnpm uses for both
registry and resolver requests. The login and token endpoints are always
served, so this works even against a resolver-only pnpr server that exposes no
registry surface.
