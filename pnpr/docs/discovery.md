---
id: discovery
title: Browsing and discovery
---

Added in: v0.1.0-alpha.11

Cross-origin browser access and upstream discovery are both off by default. Turn
them on to run a registry UI on its own origin.

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

An allowed origin must contain only an `http` or `https` scheme, a host, and an
optional port. Listing an exact origin is the whole grant: there is no wildcard.

The upstream `search` setting opts one registry into browser-facing search, and
also enables `/-/org/{scope}/package` discovery for it. pnpr applies registry
routing and access rules to the entries it returns, and uses only the upstream
credentials from its own configuration, never a browser caller's authorization
header. Discovery refuses redirects, and configured upstream headers are sent
only over HTTPS or loopback HTTP. Search totals count visible, deduplicated
results and are exact across every participating source.

## Browsing without a search term

```text
GET /-/v1/search?browse=true&size=20&from=0
GET /cargo/api/v1/crates?browse=true&per_page=20&page=1
```

`browse=true` pages through the npm packages and Cargo crates a registry hosts.
Use the [ecosystem prefix and named registry mount](ecosystems.md#addressing-an-ecosystem)
your server needs. Browse results follow registry routing and access rules,
exclude upstreams and staged publications, and use the same response format and
pagination limits as search. An empty search without `browse=true` still returns
nothing.

To bound the work one browser request can cause, pnpr rejects an upstream search
that would scan more than 2,000 upstream results or eight upstream pages, and
rejects an offset that would need a larger scan. Refine the search term when a
query reaches that limit.

## The registry directory

`GET /-/pnpr/v0/registries` lists the named registries the caller can see. It
returns `registries`, the visible `defaultRegistries` keyed by ecosystem, and
the mounted `ecosystems` with their `available` and `prefixed` flags.

Each entry carries its `name`, its `kind` (`hosted`, `upstream`, or `router`),
and its `ecosystem`; the identity is the pair `(ecosystem, name)`. Concrete
registries report their namespace `patterns` and routers their ordered
`sources`, and either field is `null` when the caller's access rules do not
permit disclosing it. Upstream addresses, credentials, storage paths, and
package access rules are never returned. Responses are private and must not be
cached.
