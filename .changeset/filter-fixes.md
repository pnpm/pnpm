---
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
---

`--filter` fixes:

- A `...pkg...` selector combined with another dependents selector, such as `--filter ...a --filter ...b...`, no longer adds the dependencies of the other selector's dependents.
- `--filter "[<since>]"` now detects changes in projects whose directory names contain non-ASCII characters. The change used to be credited to the parent project.
