---
"pacquet": patch
---

Fixed agent skill links changing targets between peer variants of the same package version. Existing links to an installed variant are recognized even when their ownership record is missing [pnpm/pnpm#17319](https://github.com/pnpm/pnpm/issues/17319).
