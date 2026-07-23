---
"pacquet": patch
---

Fixed `workspace:` dependencies failing to resolve when they point at a named workspace package whose `package.json` has no `version` (or a `null` version). Such packages are now indexed as version `0.0.0`, matching the TypeScript CLI, so specs like `workspace:*` and `workspace:0.0.0` resolve instead of failing with a misleading "no package named" error.
