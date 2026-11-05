---
"pacquet": patch
---

Running a global command such as `node` in a project that pins its version no longer hangs while pnpm installs that version. The hang kept starting new processes until the machine ran out of memory.
