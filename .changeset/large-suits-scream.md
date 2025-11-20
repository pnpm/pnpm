---
"@pnpm/read-project-manifest": patch
"pnpm": patch
---

Node.js runtime is not added to "dependencies" on `pnpm add`, if there's a `engines.runtime` setting declared in `package.json` [#10209](https://github.com/pnpm/pnpm/issues/10209).
