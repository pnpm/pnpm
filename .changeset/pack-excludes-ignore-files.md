---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` no longer put `.npmignore` and `.gitignore` files in the tarball. A `files` entry that names one still ships it.
