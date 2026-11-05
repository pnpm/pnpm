---
"pacquet": patch
---

A global command that pnpm dispatches, such as `node`, no longer dispatches again when pnpm runs it while preparing the version a project pins. That loop hung and kept starting new processes until the machine ran out of memory.
