---
"pacquet": patch
---

Sped up installs in large workspaces. Workspace `link:` targets and importer ids are now derived from the paths' suffixes under the workspace root, without walking their shared prefix [#14352](https://github.com/pnpm/pnpm/issues/14352).
