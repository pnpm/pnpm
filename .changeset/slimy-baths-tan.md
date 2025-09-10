---
"@pnpm/core": patch
---

Throw a `ABORTED_REMOVE_MODULES_DIR_NO_TTY` error if there's no TTY instead of showing the prompt to ask for confirmation to remove the modules directory and immediately exiting with code 0.
