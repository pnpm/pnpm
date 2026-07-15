---
"@pnpm/registry-access.commands": patch
"pnpm": patch
---

`pnpm owner ls` now reports authentication and authorization failures (401/403) as dedicated errors that include the registry's response body, matching `pnpm owner add`/`rm`, instead of a generic `Failed to fetch owners` message.
