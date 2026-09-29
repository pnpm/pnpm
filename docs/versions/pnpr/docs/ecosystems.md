---
id: ecosystems
title: Cargo and Python registries
---

Added in: v0.1.0-alpha.11

pnpr serves npm, Cargo, Python, and [container image](container-images.md)
registries from one instance. Every registry declares which ecosystem it serves,
and each ecosystem keeps its own protocol, its own routing, and its own default
registry.

## Addressing an ecosystem

When more than one ecosystem is configured, a path prefix selects it:

| Ecosystem | Prefix |
| --- | --- |
| npm | `/npm/` |
| Cargo | `/cargo/` |
| Python | `/pypi/` |
| Container images | `/oci/` |

`/<ecosystem>/~<name>/` addresses a named registry and `/<ecosystem>/` the
ecosystem's default. A server that configures only one ecosystem omits the
prefix entirely, so existing npm URLs keep working unchanged.

Two things sit outside the prefixes. Container image registries answer at
`/v2/` on the host root whatever else is configured, because a client derives
that path from the image reference itself. And the endpoints that belong to no
single ecosystem stay at the root: `/-/ping`, the `/-/pnpr/v0/` protocols
(resolve, verify-lockfile, shared artifacts, pipeline runs, and the
cross-ecosystem publish), and the account endpoints that mint and manage tokens.

A `/~<name>/` prefix must arrive with a literal `~`. A percent-encoded `%7E`
does not select the named registry. `~` is unreserved, so a conforming client
sends it as written.

## Declaring registries

Give a hosted or upstream registry an `ecosystem:`; npm is the default. A router
may list sources of every ecosystem, because a request only ever sees the
sources that speak its own protocol, so one router can be the default target for
all of them.

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

For a `cargo` upstream, `url` is a sparse index root; for `pypi`, a Simple API
root.

### Reusing a name across ecosystems

Group the registries under an ecosystem key to give each protocol its own
registry of the same name:

```yaml
registries:
  npm:
    internal:
      type: hosted
      packages:
        '@example/*': {}
    public:
      type: upstream
      url: https://registry.npmjs.org/
      public: true
    main:
      type: router
      sources: [internal, public]
  cargo:
    internal:
      type: hosted
    main:
      type: router
      sources: [internal]

defaultRegistry:
  npm: main
  cargo: main
```

`/npm/~internal` and `/cargo/~internal` are then independent registries. Router
sources resolve within their own group and cannot cross into another, and each
ecosystem gets its own default. Access rules, teams, upstream credentials, and
caches belong to the individual registry.

A grouped hosted registry defaults to the storage namespace
`<ecosystem>~<name>`. Set `org` explicitly to keep an existing namespace when
moving a flat registry into a group, `org: ''` included for the flat storage
root. Two hosted registries still cannot share a namespace. The flat
configuration format and its shared `defaultRegistry: main` remain supported.

## Cargo

A Cargo registry serves a sparse index at `/cargo/index/` and the crates API at
`/cargo/api/v1/crates/`, or under `/cargo/~<name>/...` for a named registry.
Point `cargo` at it:

```toml title=".cargo/config.toml"
[registries.pnpr]
index = "sparse+https://pnpr.example.com/cargo/index/"
```

`cargo publish --registry pnpr`, `cargo yank --registry pnpr`, and dependencies
with `registry = "pnpr"` then go through pnpr. The `config.json` pnpr serves
points downloads back at itself, so proxied crates are cached and verified
against the upstream index checksum.

Use a pnpr token as the registry token. `cargo` sends it as a bare
`Authorization` header, and pnpr accepts that alongside `Bearer`. A registry
that is not anonymously readable advertises `auth-required`, so `cargo` sends
the token on index and download requests too.

Crate names are compared case-insensitively, and an exact `packages:` key must
be a valid crate name.

`cargo search` works against a pnpr registry, matching on crate name over the
crates it hosts. Each result reports the crate's newest release that is not
yanked; a crate whose releases are all yanked reports its newest release.

## Python

A Python registry serves the Simple Repository API at `/pypi/simple/` (PEP 503
HTML and PEP 691 JSON, chosen by `Accept`), files at
`/pypi/files/<project>/<filename>`, and accepts uploads on the legacy API at
`/pypi/legacy/`. A named registry answers under `/pypi/~<name>/...`.

```sh
pip install --index-url https://pnpr.example.com/pypi/simple/ my-package
twine upload --repository-url https://pnpr.example.com/pypi/legacy/ dist/*
```

Use `__token__` as the username and a pnpr token as the password, the PyPI
convention.

Project names are compared PEP 503 normalized, so `My_Package`, `my.package`,
and `my-package` are one project, and exact `packages:` keys are normalized the
same way. Proxied files are fetched from the URL the upstream page lists,
verified against its `sha256`, and cached. The project list at `/pypi/simple/`
enumerates hosted projects only.

## Proxying crates.io and PyPI

Cargo and Python proxies restrict downloads and redirects to their configured
upstream origin and the existing [`routes.public`](configuration.md#routes)
allowlist. The official crates.io and PyPI upstreams also permit their download
hosts, `static.crates.io` and `files.pythonhosted.org`. A custom registry that
serves downloads from a separate host needs that host declared:

```yaml
routes:
  public:
    - registry: https://downloads.example.com/
```

Configured headers are sent only over HTTPS or loopback HTTP, and a redirect
rebuilds its headers per destination, so configured credentials stay on the
upstream origin. Upstream credentials are never sent to the separate host an
index points downloads at.

## One publish for a workspace that spans ecosystems

`PUT /-/pnpr/v0/publish` publishes packages of any ecosystem in a single
transaction. Each entry names its `ecosystem`, and an entry without one is an
npm publish document, so the body the npm batch endpoint already takes is a
valid one.

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

Every entry is authorized and verified before any of them is written. Two
different failures follow from that, so it is worth keeping them apart.

**A batch that fails a check publishes nothing.** Authorization, validation, and
the version-conflict check all run up front, and any entry that fails one of them
aborts the whole batch before a byte is written.

**A batch that passes its checks is committed as one journaled transaction.** It
lands whole or leaves nothing behind, and one interrupted by a crash is finished
on the next startup rather than staying half-published. A read that arrives while
the transaction applies can still see part of the release.

The one case that ends in a partial release is losing a write race: another
writer publishing the same file between the check and the commit. That entry is
left out and reported with `409` while the rest of the release stands, because
the bytes that won the slot are someone else's published release and overwriting
them would be the worse outcome. Republishing the batch after bumping the
conflicting version completes the release.

An [image manifest](container-images.md) whose layers are already uploaded can
ride along in the same batch. `GET /-/pnpr` advertises support as
`publish: [0]`. The npm-only `PUT /-/pnpm/v1/publish` stays where it is.

## JSR

`npm.jsr.io` is a built-in public route, like the npm registry, so JSR packages
resolve with no configuration. The built-in routes are reachable over HTTPS only.
