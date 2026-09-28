---
"@pnpm/exe": patch
"pnpm": patch
---

`@pnpm/exe` no longer ships a binary for arm64 musl Linux, such as Alpine on ARM. The published binary crashed with a segmentation fault at startup. Installing `@pnpm/exe` on that platform now fails with an error that suggests `npm install -g pnpm` or pnpm 12 instead [#10443](https://github.com/pnpm/pnpm/issues/10443).
