---
"@pnpm/package-store": patch
---

Unless an EXDEV error is thrown during hard linking, always choose hard linking for importing packages from the store.
