---
"pacquet": minor
---

`supportedArchitectures` now accepts a list of platforms, in place of the `os`, `cpu` and `libc` axes.

```yaml
supportedArchitectures:
  - linux-x64
  - darwin-arm64
  - win32-x64
```

An install prepares for the platforms the list names, and for those only. A platform reads as `<os>-<cpu>`, with a C library on Linux, as in `linux-x64-musl` or `linux-x64-manylinux_2_28`. The Rust target triple of the same machine is accepted too, so `x86_64-unknown-linux-gnu` names the platform `linux-x64` names. A Linux platform that names no C library is the glibc platform. `current` is the platform the install runs on.

The `os`, `cpu` and `libc` mapping keeps working and keeps its meaning.
