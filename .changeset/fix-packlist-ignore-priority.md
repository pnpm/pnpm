---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now honor npm-packlist's ignore-file priority: when `files` is set in package.json, `.gitignore` and `.npmignore` no longer exclude listed entries. Fixes the empty platform-package tarballs shipped in 11.12.0 and 11.13.0.
