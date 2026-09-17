---
"pacquet": minor
---

`supportedArchitectures` can now name whole platforms instead of the `os`, `cpu` and `libc` axes. A workspace that ships on Linux x64, macOS arm64 and Windows x64 prepares for those three, rather than for the six that an `os` list and a `cpu` list cross into.

```yaml
supportedArchitectures:
  - linux-x64
  - darwin-arm64
  - win32-x64
```

A platform reads as `<os>-<cpu>`, with a C library on Linux, as in `linux-x64-musl`. The Rust target triple of the same machine is accepted too, so `x86_64-unknown-linux-gnu` names the platform `linux-x64` names. A Linux platform that names no C library is the glibc platform. The `os`, `cpu` and `libc` mapping keeps working and keeps its meaning.
