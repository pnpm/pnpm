---
"pacquet": patch
---

The store-index writer's teardown no longer extends the install's tail. Closing its SQLite connection runs a WAL checkpoint — around 40ms of pure wait at the end of a cold install of a big project — so the install now starts that close as soon as the last index row is queued and waits for it only after the `.modules.yaml` and lockfile writes it can overlap with.
