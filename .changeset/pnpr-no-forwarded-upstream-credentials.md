---
"@pnpm/installing.deps-installer": minor
"@pnpm/pnpr.client": minor
"pnpm": minor
---

When resolving through a pnpr install-accelerator server, pnpm no longer forwards its own upstream registry credentials in the resolve request. Only the `Authorization` header identifying the caller to pnpr is sent. The pnpr server now selects upstream credentials from its own route policy (operator-configured upstream credential aliases), so private dependencies resolve through a pnpr-managed alias the caller is authorized to use, rather than by sending the client's registry tokens to the server.
