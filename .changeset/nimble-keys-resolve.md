---
"pacquet": patch
---

Speed up dependency resolution in large workspaces: the resolver's per-dependency cache keys now compare paths by their raw bytes instead of walking them component by component, and the importer-wide part of the shared workspace-resolution key is built once per project instead of once per dependency edge [#14352](https://github.com/pnpm/pnpm/issues/14352).
