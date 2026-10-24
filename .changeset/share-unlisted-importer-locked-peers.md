---
"pacquet": patch
---

Sped up installs that add new projects to a workspace with a peer-heavy lockfile. pnpm now reads the lockfile's locked peer versions once for all new projects [#17311](https://github.com/pnpm/pnpm/issues/17311).
