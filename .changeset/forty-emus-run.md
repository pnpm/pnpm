---
"@pnpm/releasing.commands": patch
---

Fixed pnpm pack and pnpm publish failing when prepack generates files that are included in the package and postpack cleans them up.
