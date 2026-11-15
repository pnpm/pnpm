---
"@pnpm/deps.inspection.list": patch
"pnpm": patch
"pacquet": patch
---

`pnpm list --parseable --long` now keeps `<name>@` on `link:` and `file:` dependencies whose path or peer suffix contains `@`, such as `link:../@scope/pkg`. Aliased scoped `file:` dependencies whose path contains `@` also keep `<name>@` in `pnpm list`.
