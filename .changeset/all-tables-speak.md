---
"@pnpm/config": minor
"pnpm": minor
---

Allow loading certificates from `cert`, `ca`, and `key` for specific registry URLs. E.g., `//registry.example.com/:ca=-----BEGIN CERTIFICATE-----...`. Previously this was only working via `certfile`, `cafile`, and `keyfile`.

These properties are supported in `.npmrc`, but were ignored by pnpm, this will make pnpm read and use them as well.

Related PR: [#10230](https://github.com/pnpm/pnpm/pull/10230).
