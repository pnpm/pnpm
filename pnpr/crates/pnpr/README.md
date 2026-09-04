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
