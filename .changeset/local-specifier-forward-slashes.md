---
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
---

On Windows, `pnpm add` and `pnpm update` now write relative `file:` and `link:` specifiers with forward slashes to `package.json` and the lockfile. They used to write backslashes, so the same project produced different files on Windows and on other systems [#7497](https://github.com/pnpm/pnpm/issues/7497), [#9687](https://github.com/pnpm/pnpm/issues/9687).
