---
"@pnpm/bins.resolver": patch
"pnpm": patch
---

Reject reserved manifest `bin` names (`""`, `"."`, `".."`, and scoped forms such as `@scope/..`) when resolving a package's bins. These names previously passed the bin-name guard and, when joined to the global bin directory during global remove/update/add operations, could resolve to the global bin directory itself or its parent and have it recursively deleted.
