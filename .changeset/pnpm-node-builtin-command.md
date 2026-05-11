---
"@pnpm/engine.runtime.commands": minor
"pnpm": minor
---

Added a built-in `pnpm node` command that runs Node.js using the runtime managed by pnpm, regardless of whether `node` is on PATH. The binary is resolved from the project's `node_modules/node` (when `devEngines.runtime` is installed) or from pnpm's global runtime install (`pnpm runtime set node <version> -g`), and falls back to PATH only as a last resort. Previously `pnpm node` fell back through `run` → `exec`, which depended on `node` being resolvable from PATH and could fail on Windows when the runtime shim wasn't picked up by the calling shell. A project script named `node` continues to take precedence over the built-in.
