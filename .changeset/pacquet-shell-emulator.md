---
"pacquet": minor
---

Added support for the `shellEmulator` setting. With it enabled, `pnpm run` and every lifecycle script run in a built-in POSIX shell instead of the platform's (`sh -c`, or `cmd /d /s /c` on Windows), so scripts written for `sh` behave the same on every OS. `scriptShell` is not used while the emulator is on.
