---
"@pnpm/pnpr": patch
---

Instances sharing a libSQL metadata database now accept both of two concurrent publishes to one package by the same owners: the second adds its versions and tags to what the first committed. A publish refused at its commit now keeps its archives on libSQL.
