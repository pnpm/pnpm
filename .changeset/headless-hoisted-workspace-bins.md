---
"pacquet": patch
---

A headless install (`--frozen-lockfile`) now creates the command shims for a publicly hoisted workspace package's `bin`, matching what a normal install already did and what pnpm's own headless install does. Previously those shims were missing until the next non-frozen install.
