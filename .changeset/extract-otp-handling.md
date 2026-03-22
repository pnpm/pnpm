---
"@pnpm/network.web-auth": minor
"@pnpm/auth.commands": minor
"@pnpm/releasing.commands": patch
"pnpm": minor
---

Extract shared OTP handling to `@pnpm/network.web-auth` and add OTP support to `pnpm login`. When a registry requires a one-time password during classic (CouchDB) login, pnpm now detects the `EOTP` challenge and either prompts for a code or uses the web-based authentication flow, matching npm's behavior.
