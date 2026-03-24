---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Fixed false "Command not found" errors on Windows when a command exists in PATH but exits with a non-zero code. Also fixed path resolution for `--filter` contexts where the command runs in a different package directory.
