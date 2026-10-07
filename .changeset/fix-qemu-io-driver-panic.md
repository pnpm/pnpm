---
"pacquet": patch
---

Fixed pnpm panicking with "unexpected error when polling the I/O driver" when it runs under QEMU user-mode emulation, such as a `linux/amd64` container on an Apple Silicon Mac [#16696](https://github.com/pnpm/pnpm/issues/16696).
