---
"@pnpm/build-modules": patch
"pnpm": patch
---

Defer patch errors until all patches in a group are applied, so that one failed patch does not prevent other patches from being attempted.
