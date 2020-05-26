---
"@pnpm/get-context": patch
---

When checking compatibility of the existing modules directory, start with the layout version. Otherwise, it may happen that some of the fields were renamed and other checks will fail.
