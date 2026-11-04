---
"pacquet": minor
---

`supportedArchitectures` platform lists accept platform tags, the same tags `publishConfig.variants` entries name. An entry can state floors and an ABI, such as `linux-x64-manylinux_2_28`, `darwin-arm64-macos_14_0`, `linux-x64-napi8` or `linux-x64-node127`, and any Node.js OS and CPU, such as `freebsd-x64`. The spellings accepted before still work: `glibc` or `gnu`, other names of an architecture, musl wheel baselines, and Rust target triples.
