---
"pacquet": patch
---

`pnpm install` now waits for all enabled ecosystems before publishing Cargo lockfiles and source configuration. A failed publication restores participating Cargo and Python metadata and the previous Python environment.

Cargo and Python workspace discovery now excludes configured stores and caches inside the workspace. Cached packages are not treated as workspace projects on repeated installs.
