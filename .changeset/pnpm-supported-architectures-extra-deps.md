---
"pacquet": minor
---

With `supportedArchitectures` set, `pnpm install` and `pnpm fetch` also store, without linking them, the dependencies of the archives kept for the other listed platforms. A platform added to the setting gets its archives and their dependencies on the next install. Optional dependencies a listed platform cannot run are not stored, and one that fails to download is skipped. Dependencies from git repositories are not stored for other platforms.
