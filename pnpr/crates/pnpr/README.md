# pnpr

A pnpm-compatible npm registry server, written in Rust.

Lives in the [pnpm monorepo](https://github.com/pnpm/pnpm) under `registry/`.

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
