---
"@pnpm/filter-workspace-packages": minor
"pnpm": minor
"@pnpm/config": minor
---

New option added: `test-pattern`. `test-pattern` allows to detect whether the modified files are related to tests. If they are, the dependent packages of such modified packages are not included.
