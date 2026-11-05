---
"pacquet": minor
---

With `supportedArchitectures` set, pnpm also fetches into the store the dependencies of the archives it keeps for the other listed platforms, from each variant's own entry in the lockfile, without linking them. Every install and `pnpm fetch` checks for these archives. A platform added to the setting therefore gets its archives and their dependencies on the next install, and a package already installed no longer keeps it from them. An archive the store already holds is not downloaded again.
