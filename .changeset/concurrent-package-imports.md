---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

Concurrent installs into one project now accept an already completed package import when its files match the requested package [pnpm/tasks#48](https://github.com/pnpm/tasks/issues/48).
