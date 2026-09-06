# pnpr

A pnpm-compatible npm registry server, written in Rust.

Lives in the [pnpm monorepo](https://github.com/pnpm/pnpm) under `registry/`.

## Cargo compilation cache

pnpr's experimental artifact service supports sccache's WebDAV backend.
Cargo compilation can use a local disk cache and share a remote cache between
CI and developer machines. No pnpm workspace or `package.json` is required.

Declare each cache and its permitted readers and publishers in pnpr's YAML:

```yaml
artifacts:
  enabled: true
  compilerCaches:
    acme:
      access: [ci-builder, alice, bob]
      publish: ci-builder
```

These are pnpr account names. Each caller supplies its own pnpr token.
`access` is required for both reads and writes; `publish` additionally gates
writes. An empty list denies access. The usual `$authenticated` access token
is also supported. Undeclared caches are unavailable. This policy lets CI
publish while developers only read, even when their tokens otherwise permit
registry writes. Read-only token restrictions are also enforced.

Install [sccache 0.17.0 or newer](https://github.com/mozilla/sccache) with
WebDAV support (`sccache --help` lists enabled backends). Configure the
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

`PNPR_TOKEN` must contain that caller's token;
keep it in developer credentials or CI secrets. CI uses its builder account's
token and sets `SCCACHE_WEBDAV_RW_MODE=READ_WRITE`. The pnpr policy enforces
publication permission independently of this client setting.

`SCCACHE_MULTILEVEL_CHAIN=disk,webdav` checks disk first, then pnpr, and
backfills disk on a remote hit. `SCCACHE_DIR` and `SCCACHE_CACHE_SIZE` configure
the local cache. Remote write failures do not fail a successful disk write
with sccache's default multilevel write-error policy. A running sccache daemon
retains its startup configuration; restart it with `sccache --stop-server`
after changing these settings. Use separate daemons when concurrently building
projects that need different cache settings.

Reuse depends on sccache's compilation keys, including the compiler, target,
features, flags, source inputs, and dependencies. Rust keys in sccache 0.17.0
also include the absolute compilation directory. CI and developer builds need
matching source paths, for example a common `/workspace` mount inside their
build containers, including consistent Cargo registry source paths.
`SCCACHE_BASEDIRS` does not remove this Rust restriction. Align toolchains
and build profiles for useful hit rates. Rust incremental compilation must
be disabled. System linking is not cached, and procedural macros with
undeclared filesystem inputs have correctness limitations. See
[sccache's Rust support](https://github.com/mozilla/sccache/blob/v0.17.0/docs/Rust.md).

sccache 0.17.0 also treats a multilevel cache as read-only when any tier is
read-only. Developers can read shared entries and those hits backfill disk,
but newly compiled misses are not added to disk in this mode. Builds still
succeed. For caching new local compilations, use a disk-only daemon or a
cache for which the developer has publication access. This is an upstream
[multilevel-cache limitation](https://github.com/mozilla/sccache/blob/v0.17.0/src/cache/multilevel.rs).

The WebDAV endpoint trusts pnpr and the accounts allowed to publish. Use HTTPS
outside localhost and grant publication only to trusted builds, keeping
untrusted pull-request jobs out of the shared writer credentials. Stock
sccache does not verify pnpm's signed side-effects envelopes. pnpr verifies
stored compiler entries against a digest of their cache scope, key, and bytes
before serving them; this detects storage corruption, not a malicious server
or authorized publisher.

Entries use the same configured filesystem or S3 artifact store and quota
accounting as side effects, in a separate compiler-cache namespace. A cache
key is immutable: the first successful PUT wins. `SCCACHE_WEBDAV_KEY_PREFIX`
can select a fresh namespace when needed; this does not reclaim old entries.
The experimental limits are 256 MiB per compiler entry, 1 GiB per owner/cache
name, and 10 GiB globally across artifacts. A compiler cache sharing a name
with a side-effects owner also shares its quota. There is no automatic
eviction of live compiler entries yet.

Each server accepts at most two concurrent compiler uploads, acquiring capacity
before reading request bodies. Additional uploads receive HTTP 503 with
`Retry-After: 1`. HEAD requests inspect object metadata without downloading the
entry; GET requests always verify the full content digest.

The endpoint implements GET, HEAD, and PUT at
`/-/pnpr/v0/compiler-cache/<cache>/<key>`, plus PROPFIND for virtual parent
directories. It is the subset used by sccache,
not a general WebDAV filesystem. Local builds continue to use ordinary
Cargo commands. The `sideEffectsCache` setting controls npm dependency side
effects and does not configure this integration.

## Browser registry UIs

Cross-origin browser access and upstream discovery are disabled by default. To
run a registry UI on a separate origin, list that exact origin and opt the
needed upstreams into search and organization discovery:

```yaml
cors:
  allowedOrigins:
    - https://registry-ui.example.com

registries:
  local:
    type: hosted
    access: $all
    packages:
      '@example/*': {}
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
    search: true
  main:
    type: router
    sources: [local, npmjs]

defaultRegistry: main
```

Origins must contain only an `http` or `https` scheme, host, and optional port.
The `search` setting also enables `/-/org/{scope}/package` discovery for that
upstream. pnpr applies registry routing and access rules to returned entries and
uses only the upstream credentials from its configuration, never a browser
caller's authorization header. Discovery refuses redirects and sends configured
upstream headers only over HTTPS or loopback HTTP. Search totals count only
visible, deduplicated results and are exact across every participating source.
To bound work from a single browser request, pnpr rejects upstream searches
that would scan more than 2,000 upstream results or eight upstream pages.
Offsets that would require a larger upstream scan are also rejected. Refine the
search term when a query reaches that limit.

## Cargo and Python registries

pnpr serves npm, Cargo, and Python registries from one instance. When more
than one ecosystem is configured, `/<ecosystem>/~<name>/` addresses a
registry and `/<ecosystem>/` the default registry, with `npm`, `cargo`, and
`pypi` as the codes. When only one ecosystem is configured, its prefix is
omitted and clients use the host root or `/~<name>/`.

The endpoints that belong to no single ecosystem stay at the root: `/-/ping`,
the `/-/pnpr/v0/` protocols (resolve, verify-lockfile, shared artifacts, and
the cross-ecosystem publish transaction), and the account endpoints that mint
and manage tokens.

A hosted or upstream registry declares the ecosystem it serves (`ecosystem:`,
npm by default). A router may list sources of every ecosystem: a request only
ever sees the sources that speak its protocol, so one router can be the
default target for all of them.

```yaml
registries:
  local:
    type: hosted
    packages:
      '@example/*': {}
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  crates:
    type: hosted
    ecosystem: cargo
    org: crates
    packages:
      my-crate: {}
  crates-io:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
  python:
    type: hosted
    ecosystem: pypi
    org: python
    packages:
      my-package: {}
  pypi-org:
    type: upstream
    ecosystem: pypi
    url: https://pypi.org/simple/
    public: true
  main:
    type: router
    sources: [local, npmjs, crates, crates-io, python, pypi-org]

defaultRegistry: main
```

A **Cargo** registry serves a sparse index at `/cargo/index/` and the crates
API at `/cargo/api/v1/crates/` (or `/cargo/~<name>/...` for a named
registry). Point `cargo` at it with:

```toml
# .cargo/config.toml
[registries.pnpr]
index = "sparse+https://pnpr.example.com/cargo/index/"
```

`cargo publish --registry pnpr`, `cargo yank --registry pnpr`, and
dependencies with `registry = "pnpr"` then go through pnpr. The `config.json`
pnpr serves points downloads back at itself, so proxied crates are cached and
verified against the upstream index checksum. Use a pnpr token as the registry
token; `cargo` sends it as a bare `Authorization` header and pnpr accepts that
alongside `Bearer`. A registry that is not anonymously readable advertises
`auth-required`, so `cargo` sends the token on index and download requests
too. Crate names are compared case-insensitively, and exact `packages:` keys
must be valid crate names.

A **Python** registry serves the Simple Repository API at `/pypi/simple/`
(PEP 503 HTML and PEP 691 JSON, chosen by `Accept`) and files at
`/pypi/files/<project>/<filename>`, and accepts uploads on the legacy API at
`/pypi/legacy/` (or `/pypi/~<name>/...` for a named registry):

```sh
pip install --index-url https://pnpr.example.com/pypi/simple/ my-package
twine upload --repository-url https://pnpr.example.com/pypi/legacy/ dist/*
```

Use `__token__` as the username and a pnpr token as the password, the PyPI
convention. Project names are compared PEP 503 normalized, so `My_Package`,
`my.package`, and `my-package` are one project, and exact `packages:` keys are
normalized the same way. Proxied files are fetched from the URL the upstream
page lists, verified against its `sha256`, and cached. The project list at
`/pypi/simple/` enumerates hosted projects only. Upstream credentials are sent
only to the upstream's own origin, never to the separate host an index points
downloads at.

### One publish for a workspace that spans ecosystems

`PUT /-/pnpr/v0/publish` publishes packages of any ecosystem in a single
transaction. Each entry names its `ecosystem` — absent means npm, so the body
the npm batch endpoint takes is already a valid one — and carries what that
ecosystem's own publish endpoint takes, with the binary parts base64-encoded:

```json
{
  "packages": [
    { "name": "@acme/ui", "versions": {}, "_attachments": {} },
    { "ecosystem": "cargo", "metadata": { "name": "acme", "vers": "0.1.0" },
      "archive": "<base64 .crate>" },
    { "ecosystem": "pypi", "name": "acme", "version": "0.1.0",
      "filetype": "bdist_wheel", "filename": "acme-0.1.0-py3-none-any.whl",
      "content": "<base64 wheel>" }
  ]
}
```

Every entry is authorized and verified before any of them is written, and the
write is one journaled transaction: the release either lands whole or leaves
nothing behind, and one interrupted by a crash is completed on the next
startup rather than staying half-published. A read that lands while the
transaction applies can still see some of the release and not the rest. If
another writer has already published one of the files, that package is left
out and reported with `409`, and the rest of the release stays: the bytes that
won the slot are someone else's published release. The endpoint sits outside the per-ecosystem prefixes because the
batch belongs to no single ecosystem; the npm-only `PUT /-/pnpm/v1/publish`
stays where it is. `GET /-/pnpr` advertises support as `publish: [0]`.

Cargo and Python proxies restrict downloads and redirects to their configured
upstream origin and the existing `routes.public` allowlist. The official
crates.io and PyPI upstreams also permit their respective download hosts,
`static.crates.io` and `files.pythonhosted.org`. For a custom registry using a
separate download host, declare it explicitly:

```yaml
routes:
  public:
    - registry: https://downloads.example.com/
```

Configured headers are sent only over HTTPS or loopback HTTP. Redirects rebuild headers for each destination, so configured credentials stay
on the upstream origin.

## License

Source-available under the [PolyForm Shield License 1.0.0](../../LICENSE.md) —
**not** open source. You may run, modify, and self-host `pnpr` for any purpose
except providing a product that competes with `pnpr` (or with a product the
licensor provides using it). Commercial / non-compete licenses are available
from Zoltan Kochan (<https://kochan.io>).

This is the only part of the pnpm monorepo that is not MIT licensed.

Contributions to `pnpr/` are accepted under separate terms — see
[`CONTRIBUTING.md`](../../CONTRIBUTING.md).

## Trademark notice

pnpr is not affiliated with, endorsed by, or sponsored by npm, Inc., GitHub, or
Microsoft. "npm" is a trademark of npm, Inc., used here only to describe
compatibility with the npm registry protocol.
