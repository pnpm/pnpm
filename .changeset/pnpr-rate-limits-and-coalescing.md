---
"@pnpm/pnpr": minor
---

Package write requests now check permissions before buffering large request bodies. Upstream packument fetches are coalesced across concurrent requests. Login endpoints enforce rate limits, and incoming requests support a configurable `requestTimeout` [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
