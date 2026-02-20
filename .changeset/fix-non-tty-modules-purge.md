---
"@pnpm/core": patch
"pnpm": patch
---

Fixed modules directory purge hanging in non-interactive environments such as Docker builds, CI pipelines, and scripted subprocesses.

Previously, when the modules directory needed to be purged and reinstalled, `pnpm install` would show an interactive confirmation prompt. In non-TTY environments (Docker, piped stdin, automated scripts), this caused the process to hang indefinitely waiting for user input that would never come.

Now, the confirmation prompt is only shown in interactive terminals (when stdin is a TTY and not in CI). In all other cases, the purge proceeds automatically with an informational log message.

Fixes #6778, #9166, #8654, #8085