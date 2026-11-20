---
"@pnpm/pnpr": patch
---

pnpr now rejects nested ecosystem groups in `registries`. Declare registries directly under `registries` with unique names and select their protocol with `ecosystem`. Hosted registries use the default storage namespace when `org` is omitted.
