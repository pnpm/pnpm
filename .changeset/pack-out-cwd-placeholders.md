---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r pack --out` now expands `%s` and `%v` placeholders only in the `--out` template. Literal `%s` and `%v` segments in the working directory path stay unchanged.
