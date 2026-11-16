---
"pnpm": patch
"pacquet": patch
---

Project `engines.node` checks now use the Node.js runtime linked for the project. A locked runtime range uses its resolved version. Production installs ignore a runtime declared only in `devEngines.runtime`.
