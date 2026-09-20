---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm audit --fix=update` now fixes vulnerabilities in dependencies declared through an npm alias, such as `"foo": "npm:vulnerable-pkg@1.0.0"`. The aliased specifier is updated to the patched version, keeping the `npm:` alias shape. Previously these dependencies were silently skipped. An aliased pin owned by a `packageExtensions` entry, a `readPackage` hook, or an override now reports the same guidance as other hook-owned pins [#15155](https://github.com/pnpm/pnpm/issues/15155).
