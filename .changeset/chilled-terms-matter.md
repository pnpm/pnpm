---
"@pnpm/core": patch
---

Change the install error message when a lockfile is wanted but absent to
indicate the wanted lockfile is absent, not present. This now reflects
the actual error.
