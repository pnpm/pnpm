---
"pacquet": minor
---

Added support for the `shellEmulator` setting. With it enabled, the scripts `pnpm run` executes, a project's own lifecycle scripts, and dependencies' build scripts run in a built-in POSIX shell instead of the platform's (`sh -c`, or `cmd /d /s /c` on Windows), so scripts written for `sh` behave the same on every OS. `scriptShell` is not used while the emulator is on.
