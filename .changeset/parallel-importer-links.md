---
"pacquet": patch
---

Workspace installs link projects' `node_modules` concurrently: the per-project symlink and bin passes now run in parallel across all workspace projects, cutting the linking step roughly in half on many-project workspaces (~0.25 s on a 60-project workspace).
