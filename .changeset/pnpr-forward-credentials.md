---
"@pnpm/installing.deps-installer": minor
"@pnpm/pnpr.client": minor
"pnpm": minor
---

The pnpr install accelerator now forwards the caller's per-registry credentials on `POST /v1/install`, so it can resolve, verify, and fetch private dependencies from external registries as the caller. The client sends an `Authorization` header identifying itself to the pnpr server plus an `authHeaders` map of the registry tokens (built with `@pnpm/network.auth-header`), and the server threads those credentials through resolution and fetch instead of reaching the registry anonymously. Externally-resolved private content carries no pnpr access policy, so the server gates it per user against the owning registry — serving a cache hit only to a user the registry has cleared — and re-checks access (clearing it on a `401`/`403`) rather than letting the store's possession of the bytes authorize anyone. Packages the registry serves anonymously are classified public once (globally) and then served to everyone without per-user access checks, so a registry that mixes public and private packages doesn't pay the per-user cost for its public ones.
