---
"pacquet": patch
---

Sped up dependency resolution when there is no lockfile. pnpm now requests the metadata of a package's whole dependency subtree as soon as the package resolves, instead of one level at a time, so a cold resolve pays far fewer network round trips.
