---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
---

After relaying a signal to a script, pnpm keeps waiting for a process in the script's process group whose main thread has exited while its other threads still run. Linux reports such a process as a zombie, so the wait used to end before those threads finished [pnpm/tasks#56](https://github.com/pnpm/tasks/issues/56).
