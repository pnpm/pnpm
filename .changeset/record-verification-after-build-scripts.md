---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

The lockfile verification result is now recorded once the install has finished, rather than as soon as the lockfile passes. The log is read by the next install and the newest record for a lockfile wins, so an install should have the last word on the lockfile it verified: recording before the build phase let a dependency's lifecycle script append over pnpm's verdict, and left a "verified" record behind for an install that went on to fail.
