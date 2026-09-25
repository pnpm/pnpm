---
"@pnpm/exe": patch
"pnpm": patch
"pacquet": patch
---

`pn`, `pnpx`, `pnx`, and `pnpm` now run when Git Bash, MSYS2, or Cygwin launches them through a Windows path such as `C:\Users\me\node_modules\pnpm\pn`. The aliases used to fail to find the pnpm installed beside them, or hand the call to an unrelated one [#14884](https://github.com/pnpm/pnpm/issues/14884).
