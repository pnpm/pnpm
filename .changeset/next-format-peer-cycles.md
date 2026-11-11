---
"pacquet": patch
---

Fixed writing `lockfile.format: v13` for dependency cycles with missing, optional, or transitive peers. Frozen installs also accept unchanged injected packages whose peers use catalogs. With `--no-optional`, frozen installs reject required catalog children recorded only as optional dependencies [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).
