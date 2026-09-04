---
"pacquet": patch
---

Sped up restoring a workspace with many projects from a warm global virtual store. Linking a project's bins no longer rereads dependency manifests that the store index already holds [#14540](https://github.com/pnpm/pnpm/issues/14540).
