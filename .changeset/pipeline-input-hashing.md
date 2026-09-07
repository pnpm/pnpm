---
"pacquet": patch
---

`pnpm pipeline` now rejects symlinked inputs when computing task cache keys. Unreadable input files now produce an error. Non-UTF-8 input filenames now produce an error. Unix filenames containing backslashes are now hashed as literal paths.

Projects containing Git submodules and tasks depending on them bypass caching and still execute.
