---
"pacquet": patch
---

Fixed `pnpm store status` reporting packages with peer dependencies as modified. The check now verifies each package in the peer-suffixed directory where the install placed it.

Skipped optional dependencies are no longer reported as modified either.
