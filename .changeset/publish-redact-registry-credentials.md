---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm publish` no longer prints credentials to its log when the target registry is configured with inline `user:pass@` credentials (e.g. `registry=https://user:pass@example.com/`). They are now redacted from the "publishing to registry" line.
