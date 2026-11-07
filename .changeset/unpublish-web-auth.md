---
"@pnpm/registry-access.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm unpublish` now offers browser-based authentication when the account's two-factor authentication uses a security key. Before, it asked for a one-time password in that case [#17832](https://github.com/pnpm/pnpm/issues/17832).
