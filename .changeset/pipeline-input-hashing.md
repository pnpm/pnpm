---
"pacquet": patch
---

`pnpm pipeline` now reports symlinked inputs, unreadable files, and non-UTF-8 filenames when computing task cache keys. Unix filenames containing backslashes are now hashed as literal paths.

Projects containing Git submodules and tasks depending on them bypass caching and still execute.
