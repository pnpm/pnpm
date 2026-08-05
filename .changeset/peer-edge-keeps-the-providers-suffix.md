---
"pacquet": patch
---

A peer dependency is now recorded in the lockfile at the version and peer suffix the peer provider actually resolved to. Peers whose provider carried peer suffixes of its own could be recorded against a package instance that no importer installs, leaving an unreachable entry in `snapshots:` and a peer bound to the wrong instance [#13320](https://github.com/pnpm/pnpm/issues/13320).
