---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm audit --fix` now updates vulnerable packages in a single project that sets `updateConfig.ignoreDependencies`. It used to leave them on the vulnerable version.
