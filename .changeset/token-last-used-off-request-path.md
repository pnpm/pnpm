---
"@pnpm/pnpr": patch
---

Token-authenticated requests are now handled without waiting for the token's last-used time to be written to the auth database. Every such request waited for that write, a database round trip on a shared or replicated database. The server now records a token's last use in the background, at most once a minute per token [pnpm/tasks#194](https://github.com/pnpm/tasks/issues/194).
