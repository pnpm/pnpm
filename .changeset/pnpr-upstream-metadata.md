---
"@pnpm/pnpr": patch
---

pnpr now fetches the package document when an upstream answers a version manifest request with HTTP 501. Scoped version manifests and attestations use an encoded package name.

Package resolution and lockfile verification now send custom upstream headers on npm metadata requests. Importing a registry with credentials requires HTTPS or loopback HTTP, including IPv4-mapped loopback addresses.
