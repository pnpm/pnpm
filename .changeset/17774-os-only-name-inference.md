---
"@pnpm/config.package-is-installable": patch
"pnpm": patch
"pacquet": patch
---

An optional dependency that declares no `os`, `cpu`, or `libc` field is no longer skipped because its name contains only an operating system word, such as `get-windows` on macOS. pnpm now infers its platform from the name only when the name also has a cpu or libc token, such as `-win32-x64` [pnpm/pnpm#17774](https://github.com/pnpm/pnpm/issues/17774).
