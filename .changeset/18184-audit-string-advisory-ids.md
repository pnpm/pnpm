---
"@pnpm/deps.compliance.audit": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` now reports advisories whose `id` is a string. Such advisories were dropped, and `pnpm audit` reported no known vulnerabilities [#18184](https://github.com/pnpm/pnpm/issues/18184).
