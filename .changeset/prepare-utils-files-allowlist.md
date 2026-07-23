---
"@pnpm/prepare": patch
"@pnpm/prepare-temp-dir": patch
"pnpm": patch
---

Declare `files` allowlists in `@pnpm/prepare` and `@pnpm/prepare-temp-dir` so their compiled `lib/` payload no longer depends on workspace ignore rules at pack time [#13164](https://github.com/pnpm/pnpm/issues/13164).
