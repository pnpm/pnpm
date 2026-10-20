---
"pacquet": patch
---

Installs that resolve dependencies in large workspaces finish faster. The peer dependency check that runs after the lockfile is written now reads each linked workspace package once.
