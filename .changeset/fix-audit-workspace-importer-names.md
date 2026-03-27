---
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
---

`pnpm audit` excludes workspace package directories from the audit payload, avoiding false positive advisories when a directory name matches a vulnerable package [pnpm/pnpm#11101](https://github.com/pnpm/pnpm/issues/11101).
