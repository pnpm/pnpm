---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add` no longer warns that a gist dependency was replaced from a different source when the new specifier names the same gist in another form, such as `gist:<id>` and `https://gist.github.com/<user>/<id>`.
