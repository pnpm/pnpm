---
"pacquet": patch
---

Resolving a workspace now produces the same `pnpm-lock.yaml` every time. Projects' initial dependency waves resolved concurrently, and racing for a package's shared children claim left a varying number of occurrence nodes behind, which was enough to bind a peer dependency to a different — still valid — version between two installs of the same input.
