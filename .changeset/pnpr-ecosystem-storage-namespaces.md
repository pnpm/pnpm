---
"@pnpm/pnpr": patch
---

Hosted storage namespaces now include the ecosystem (`default/<ecosystem>/` and `orgs/<org>/<ecosystem>/`), so hosted registries of different ecosystems can share an `org` and grouped registries default their `org` to their own name. Multi-component OCI repository names now map each `/`-separated component to its own directory segment.
