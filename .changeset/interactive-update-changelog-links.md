---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update --interactive` now links npm packages to the changelog for their target version on
npmx instead of linking to the package homepage.
