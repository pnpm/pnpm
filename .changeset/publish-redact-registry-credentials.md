---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm publish` no longer prints credentials when the target registry is configured with inline `user:pass@` credentials (e.g. `registry=https://user:pass@example.com/`). They are now redacted both from the "publishing to registry" line and from the OIDC (trusted publishing) failure messages.
