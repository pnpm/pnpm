---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

An optional peer dependency is no longer resolved from another workspace project's package when the project provides one of that package's own peers at a version it rejects. This avoids bogus unmet peer errors [#13989](https://github.com/pnpm/pnpm/issues/13989).
