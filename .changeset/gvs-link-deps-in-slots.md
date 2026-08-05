---
"pacquet": patch
---

A package whose peer dependency is satisfied by a `link:` no longer fails at runtime with `Cannot find module` under `enableGlobalVirtualStore`, and two projects that link different directories no longer share one virtual-store slot.
