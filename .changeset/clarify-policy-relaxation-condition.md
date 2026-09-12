---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

The supply-chain policy error now suggests `relax the policy that flagged them` only after a fresh resolution still fails and the affected packages are trusted. Previously it followed `If the changes look expected`, so `Alternatively` read as the action for the unexpected case [#14411](https://github.com/pnpm/pnpm/issues/14411).
