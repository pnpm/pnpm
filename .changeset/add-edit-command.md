---
"@pnpm/patching.commands": minor
"pnpm": minor
---

Added the `edit` command to open an installed package's folder in the default text editor, automatically breaking store hard links to prevent CAS corruption, and running rebuild after the editor closes.
