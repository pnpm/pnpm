---
"@pnpm/global.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm update --global --latest` failing with a 404 error when a linked local package is installed globally. Local packages (`link:`/`file:`) keep their spec during a global update instead of being resolved from the registry. See pnpm/pnpm#12854.
