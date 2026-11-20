---
"@pnpm/exec.commands": minor
"pnpm": minor
"pacquet": minor
---

Added support for executing multiple scripts matching a RegExp passed to `pnpm run` (e.g., `pnpm run "/^build:.*/"`), running matched scripts in deterministic lexicographical order. Restored the `--sequential` (`-s`) CLI option for `pnpm run`, which forces `workspaceConcurrency` to 1 so that matched scripts run sequentially one by one across and within packages.
