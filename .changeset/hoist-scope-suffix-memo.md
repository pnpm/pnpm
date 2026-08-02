---
"pacquet": patch
---

Resolving a workspace whose dependency chains are deep is faster: deciding which missing peer dependencies another project's resolution already covers now answers once per shared chain segment instead of once per report.
