---
"@pnpm/pnpr": patch
---

Fixed a shared build artifact publication failing with a broken pipe when another publisher stored the same blob at the same moment.
