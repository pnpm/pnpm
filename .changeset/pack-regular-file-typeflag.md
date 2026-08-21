---
"pacquet": patch
---

`pnpm pack` writes tar entries in the POSIX ustar header form npm uses — `ustar\0` magic and the explicit `0` regular-file typeflag — instead of the GNU form with a NUL typeflag, which strict tar readers such as publint mistake for the end-of-archive marker [#13924](https://github.com/pnpm/pnpm/issues/13924).
