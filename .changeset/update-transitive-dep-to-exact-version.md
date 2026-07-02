---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fixed `pnpm update <dep>@<version>` installing the highest in-range version instead of the requested one when `<dep>` is only present as a transitive dependency. The requested exact version is now preferred during resolution. Closes pnpm/pnpm#12744.
