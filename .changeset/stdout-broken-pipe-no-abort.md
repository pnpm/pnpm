---
"pacquet": patch
---

pnpm no longer aborts with a core dump when the reader of its stdout has exited, as in `pnpm list | head -1`. The command finishes with its usual exit code. A write to stdout that fails for another reason, such as a full disk, ends the command with exit code 1 [pnpm/tasks#132](https://github.com/pnpm/tasks/issues/132).
