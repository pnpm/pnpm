---
id: compiler-cache
title: Cargo compilation cache
---

Added in: v0.1.0-alpha.11

pnpr's artifact service speaks [sccache](https://github.com/mozilla/sccache)'s
WebDAV backend, so a Cargo compilation cache can be shared between CI and
developer machines. No pnpm workspace or `package.json` is involved.

## Declaring a cache

```yaml
artifacts:
  enabled: true
  compilerCaches:
    acme:
      access: [ci-builder, alice, bob]
      publish: ci-builder
```

These are pnpr account names, and each caller supplies its own pnpr token.
`access` is required for reads and writes alike; `publish` additionally gates
writes. An empty list denies access, and the `$authenticated` token works here
too. An undeclared cache is unavailable.

This is what lets CI publish while developers only read, even where their tokens
otherwise permit registry writes. Read-only token restrictions are enforced as
well.

## Configuring sccache

Install [sccache 0.17.0 or newer](https://github.com/mozilla/sccache) with
WebDAV support (`sccache --help` lists the enabled backends), then set the
environment before starting sccache or invoking Cargo:

```sh
export RUSTC_WRAPPER=sccache
export CARGO_INCREMENTAL=0
export SCCACHE_MULTILEVEL_CHAIN=disk,webdav
export SCCACHE_WEBDAV_ENDPOINT=https://cache.example.com/-/pnpr/v0/compiler-cache/acme
export SCCACHE_WEBDAV_TOKEN="$PNPR_TOKEN"
export SCCACHE_WEBDAV_RW_MODE=READ_ONLY

cargo build
sccache --show-stats
```

`PNPR_TOKEN` holds that caller's token; keep it in developer credentials or CI
secrets. CI uses its builder account's token and sets
`SCCACHE_WEBDAV_RW_MODE=READ_WRITE`. The pnpr policy enforces publication
permission independently of that client setting.

`SCCACHE_MULTILEVEL_CHAIN=disk,webdav` checks disk first, then pnpr, and
backfills disk on a remote hit. `SCCACHE_DIR` and `SCCACHE_CACHE_SIZE` configure
the local cache. With sccache's default multilevel write-error policy, a remote
write failure does not fail a successful disk write. A running sccache daemon
keeps its startup configuration, so restart it with `sccache --stop-server`
after changing these settings, and use separate daemons for projects that need
different cache settings concurrently.

## What determines a hit

Reuse depends on sccache's compilation keys: the compiler, target, features,
flags, source inputs, and dependencies. Rust keys in sccache 0.17.0 also include
the absolute compilation directory, so CI and developer builds need matching
source paths, for example a common `/workspace` mount inside their build
containers, with consistent Cargo registry source paths. `SCCACHE_BASEDIRS` does
not lift that Rust restriction.

Align toolchains and build profiles for useful hit rates. Rust incremental
compilation must be off. System linking is not cached, and procedural macros
with undeclared filesystem inputs have correctness limitations. See
[sccache's Rust support](https://github.com/mozilla/sccache/blob/v0.17.0/docs/Rust.md).

sccache 0.17.0 also treats a multilevel cache as read-only when any tier is
read-only. Developers can read shared entries, and those hits backfill disk, but
newly compiled misses are not added to disk in that mode. Builds still succeed.
To cache new local compilations, use a disk-only daemon or a cache the developer
may publish to. This is an upstream
[multilevel-cache limitation](https://github.com/mozilla/sccache/blob/v0.17.0/src/cache/multilevel.rs).

## Storage, limits, and trust

Entries use the configured filesystem or S3 artifact store and the same quota
accounting as [side effects](shared-side-effects-cache.md), in a separate
compiler-cache namespace. A cache key is immutable: the first successful `PUT`
wins. `SCCACHE_WEBDAV_KEY_PREFIX` can select a fresh namespace, though that does
not reclaim old entries.

The experimental limits are 256 MiB per compiler entry, 1 GiB per owner or cache
name, and 10 GiB globally across artifacts. A compiler cache sharing a name with
a side-effects owner shares its quota. Live compiler entries are not evicted
automatically yet.

Each server accepts at most two concurrent compiler uploads, acquiring capacity
before reading request bodies. Further uploads get `503` with `Retry-After: 1`.
`HEAD` inspects object metadata without downloading the entry; `GET` always
verifies the full content digest.

The WebDAV endpoint trusts pnpr and the accounts allowed to publish. Use HTTPS
outside localhost, and grant publication only to trusted builds, keeping
untrusted pull-request jobs out of the shared writer credentials. Stock sccache
does not verify pnpm's signed side-effects envelopes. pnpr verifies stored
compiler entries against a digest of their cache scope, key, and bytes before
serving them, which detects storage corruption, not a malicious server or an
authorized publisher.

The endpoint implements `GET`, `HEAD`, and `PUT` at
`/-/pnpr/v0/compiler-cache/<cache>/<key>`, plus `PROPFIND` for virtual parent
directories. It is the subset sccache uses, not a general WebDAV filesystem.
Local builds keep using ordinary Cargo commands. The pnpm
[`sideEffectsCache`](/settings/build#sideeffectscache) setting governs npm
dependency side effects and does not configure this integration.
