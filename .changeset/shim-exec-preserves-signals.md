---
"pacquet": patch
---

Command shims on POSIX again `exec` a target that has no interpreter — a runtime binary such as the managed Node.js, or any bin without a shebang — instead of waiting on it. A shim that waited reported a target killed by a signal as exit code `128+N` (for example `137` for `SIGKILL`), so callers that distinguish a signal death from an exit code, such as CI runners and process supervisors, saw the wrong outcome.
