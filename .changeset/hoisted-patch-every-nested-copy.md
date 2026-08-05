---
"pacquet": patch
---

Fixed patched dependencies being applied to only one copy of a package under `nodeLinker: hoisted`. When a version conflict kept a patched package out of the root `node_modules`, the hoisted layout nested a copy of it under each consumer that needed it, but only the first copy was patched — every other copy silently ran the unpatched code the patch existed to replace. The same gap applied to a reinstall served from the side-effects cache. Every copy is now patched, matching `nodeLinker: isolated` and pnpm's behavior.
