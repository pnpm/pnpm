---
"pacquet": patch
---

On Windows, `pnpm install` no longer fails with `os error 123` when a dependency declares a specifier that pnpm reads as a local path but that is not a valid file name, such as a Yarn `patch:` specifier. pnpm now warns about the missing directory, as it does on other platforms. When reading a `package.json` fails, the error now names the file [#16590](https://github.com/pnpm/pnpm/issues/16590).
