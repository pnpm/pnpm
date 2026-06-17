---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed `pnpm update` rewriting a `workspace:` dependency that points at a local path (e.g. `workspace:../packages/foo/dist`) into a normalized `link:` or version-range specifier. Such specifiers are now preserved verbatim when the workspace protocol is preserved [#3902](https://github.com/pnpm/pnpm/issues/3902).
