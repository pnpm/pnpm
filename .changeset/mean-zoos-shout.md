---
"@pnpm/resolve-dependencies": major
---

In case of leaf dependencies (dependencies that have no prod deps or peer deps), we only ever need to analyze one leaf dep in a graph, so the nodeId can be short and stateless, like the package ID.
