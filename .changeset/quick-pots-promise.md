---
"@pnpm/plugin-commands-deploy": patch
"@pnpm/directory-fetcher": patch
"pnpm": patch
---

Reverting a change shipped in v8.7 that caused issues with the `pnpm deploy` command and "injected dependencies" [#6943](https://github.com/pnpm/pnpm/pull/6943).

