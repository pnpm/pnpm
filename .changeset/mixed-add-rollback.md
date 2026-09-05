---
"pacquet": patch
---

Failed `pnpm add` operations that include crates now restore the Node.js and Cargo manifests and lockfiles. These operations previously could leave one ecosystem partially updated.
