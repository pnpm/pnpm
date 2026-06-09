---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fix lockfile churn where `transitivePeerDependencies` shifted between packages when unrelated dependencies were upgraded.
