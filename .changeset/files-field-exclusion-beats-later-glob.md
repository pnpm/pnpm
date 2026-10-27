---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now apply a `files` entry that names a file after the glob and directory entries, as npm does. A file excluded by name, such as `!dist/index.js`, stays out even when a later `dist/**` matches it. A file named by its path, such as `lib/secret.js`, ships even when a later `!lib/*.js` matches it.
