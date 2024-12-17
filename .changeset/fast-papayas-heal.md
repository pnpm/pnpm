---
"@pnpm/plugin-commands-doctor": minor
"pnpm": minor
---

Symlink are not supported in the exFAT driver, and installation of dependencies will result in errors. The doctor command checks if the current drive is exFAT and gives a warning message.
