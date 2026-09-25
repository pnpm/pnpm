---
"@pnpm/pnpr": minor
---

The pnpr resolver no longer connects to loopback, private, link-local, or other non-public addresses, including the `169.254.169.254` cloud metadata endpoint. The check applies to the address each connection is made to, so an allowlisted registry host that resolves to an internal address is refused too. Addresses of pnpr's own `public_url` host stay reachable. List any other non-public network the resolver must reach, such as an internal upstream registry's, under `routes.allowedPrivateNetworks`. Transitive git and tarball dependencies are now checked against the fetch allowlist before pnpr contacts their host [#12705](https://github.com/pnpm/pnpm/issues/12705).
