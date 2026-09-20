---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm audit --fix=update` now fixes vulnerabilities in dependencies declared through an npm alias. An aliased specifier such as `"foo": "npm:vulnerable-pkg@1.0.0"` is updated to the patched version, keeping the alias [#15155](https://github.com/pnpm/pnpm/issues/15155).
