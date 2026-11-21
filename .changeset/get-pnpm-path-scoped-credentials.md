---
"pacquet": patch
"get-pnpm": patch
---

When the `pnpm` package downloads its native binary on first run, credentials configured for a registry path, such as `//registry.example.com/org-a/npm/:_authToken`, are now sent only to that path. They were sent to any path on the same host.
