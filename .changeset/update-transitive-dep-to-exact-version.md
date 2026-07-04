---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/resolving.resolver-base": patch
"pnpm": patch
---

Fixed `pnpm update <dep>@<version>` installing the highest in-range version instead of the requested one when `<dep>` is only present as a transitive dependency. The requested exact version is now preferred during re-resolution, while unrelated packages keep their lockfile pins. Closes pnpm/pnpm#12744.
