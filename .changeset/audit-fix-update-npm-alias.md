---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm audit --fix=update` now fixes vulnerabilities in dependencies declared through an npm alias. A specifier such as `"foo": "npm:vulnerable-pkg@1.0.0"` moves to the patched version and keeps the alias. Versions pinned with a leading `=` are fixed as well [#15155](https://github.com/pnpm/pnpm/issues/15155).
