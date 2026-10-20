---
"pacquet": patch
---

Sped up the peer dependency check at the end of an install in workspaces whose projects depend on each other. pnpm now checks the peers of linked workspace projects against the manifests it already read, without reading each `package.json` again.
