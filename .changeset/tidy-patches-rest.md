---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun` no longer reports "Patches were modified" when a patch file has a newer modification time but unchanged content, such as after a copy or `touch` [#16988](https://github.com/pnpm/pnpm/issues/16988).
