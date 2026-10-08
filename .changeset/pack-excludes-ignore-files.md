---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` no longer put `.npmignore` and `.gitignore` files in the tarball, unless a `files` entry matches the file itself.
