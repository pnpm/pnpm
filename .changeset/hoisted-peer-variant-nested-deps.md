---
"pacquet": patch
---

With `nodeLinker: hoisted`, when two peer variants of one package version compete for a top-level `node_modules` slot, the variant found under a direct dependency wins over one found deeper in the graph. The winning variant's own `node_modules` keeps the dependencies it resolved at a version other than the top-level one. Before, the deeper variant could take the slot and that nested dependency was missing, so the package loaded a version of it that its dependent did not expect.
