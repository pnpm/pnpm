---
"pacquet": patch
---

Warm installs no longer read the `package.json` of every git-hosted dependency. pnpm now stores the manifest of a git-hosted package in the store index when it fetches the package.
