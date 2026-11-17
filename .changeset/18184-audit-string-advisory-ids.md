---
"@pnpm/deps.compliance.audit": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` now reports advisories that have a string `id`, as some registries send. It used to drop them and report no known vulnerabilities [#18184](https://github.com/pnpm/pnpm/issues/18184).
