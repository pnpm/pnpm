---
"@pnpm/pnpr": patch
---

`npm search` against an upstream with `search: true` no longer fails with 400 for broad terms. The fetch budget now bounds what pnpr downloads instead of refusing over the total the upstream advertises, which for npmjs's full-text search is in the tens of thousands for almost any term. A source too large to exhaust is truncated, and the results left beyond the budget keep `total` approximate.
