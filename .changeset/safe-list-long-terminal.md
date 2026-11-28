---
"@pnpm/deps.inspection.list": patch
"pnpm": patch
---

`pnpm list --long` and `pnpm why --long` now remove terminal control characters from the description, repository, and homepage of each package.
