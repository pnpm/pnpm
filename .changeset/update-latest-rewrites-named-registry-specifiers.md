---
"pacquet": patch
---

`pnpm update --latest` now rewrites dependencies using a named registry alias. The manifest keeps the alias prefix and the range operator it declared, so `gh:1.0.0` becomes `gh:2.0.0` and `gh:@acme/foo@^1.0.0` becomes `gh:@acme/foo@^2.0.0`, instead of being left at the old version [pnpm/pnpm#13393](https://github.com/pnpm/pnpm/issues/13393).
