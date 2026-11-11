---
"pacquet": patch
---

Fixed writing `lockfile.format: v13` for dependency cycles with missing, optional, or transitive peers. Frozen installs also accept unchanged injected packages whose peers use catalogs.
