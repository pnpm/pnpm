---
"@pnpm/os.env.path-extender-windows": patch
"pnpm": patch
---

`pnpm setup` no longer garbles non-ASCII characters in existing Windows `Path` entries [#6346](https://github.com/pnpm/pnpm/issues/6346).
