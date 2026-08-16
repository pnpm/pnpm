---
"pacquet": patch
---

Installs with a cold cache are significantly faster: lockfile verification no longer delays resolution or downloads and re-checks far less data over the network, downloaded packages are linked while the remaining downloads are still in flight, and re-verification is skipped for an already-installed lockfile even after the cache directory was cleared.
