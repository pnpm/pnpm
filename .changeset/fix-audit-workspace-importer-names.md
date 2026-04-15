---
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
---

Fixed `pnpm audit` misreporting advisories for workspace packages whose directory names match a vulnerable package by using the workspace manifest name, rather than the importer path, in the generated audit payload [#11101](https://github.com/pnpm/pnpm/issues/11101).
