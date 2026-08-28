---
"pacquet": patch
---

Installs that run no build scripts finish faster: the post-build bin pass no longer re-reads every project's dependency manifests when no lifecycle script, patch, or side-effects overlay changed anything.
