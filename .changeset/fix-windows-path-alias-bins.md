---
"@pnpm/exe": patch
"pnpm": patch
"pacquet": patch
---

`pn`, `pnpx`, and `pnx` now run the pnpm installed beside them when Git Bash, MSYS2, or Cygwin launches them through a native Windows path such as `C:\Users\me\node_modules\pnpm\pn`. They used to fail to find it, or hand the call to an unrelated pnpm. The `pnpm` script that stands in until the native binary is installed reads its own path the same way [#14884](https://github.com/pnpm/pnpm/issues/14884).
