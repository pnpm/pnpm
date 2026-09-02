---
"pacquet": patch
---

Sped up installs in large workspaces: the fast lockfile-update check no longer compares every project against every lockfile entry (or copies the whole lockfile before discovering a change needs the resolver), project ordering uses faster hashing, and the version-preference table builds in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).
