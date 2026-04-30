---
"@pnpm/config.package-is-installable": patch
"pnpm": patch
---

Fix negated `os` / `cpu` entries (e.g. `["!win32"]`) being incorrectly rejected when `supportedArchitectures` expands to multiple platforms [#11375](https://github.com/pnpm/pnpm/pull/11375).