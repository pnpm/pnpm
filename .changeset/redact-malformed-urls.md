---
"@pnpm/pnpr": patch
---

pnpr now redacts malformed URLs in logs as `<scheme>://redacted` when a `?` or `#` appears before `@` [pnpm/tasks#301](https://github.com/pnpm/tasks/issues/301).
