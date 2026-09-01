---
"pacquet": patch
---

Speed up installs in large workspaces by doing the workspace dependency-graph work once: the graph's edges now resolve in parallel, the topological sort runs over borrowed paths instead of cloned ones, and a full unfiltered install reuses the cycle report its project selection already computed instead of rebuilding the graph to search again [#14352](https://github.com/pnpm/pnpm/issues/14352).
