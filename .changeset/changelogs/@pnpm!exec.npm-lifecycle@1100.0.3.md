## 1100.0.3

### Patch Changes

- A lifecycle script run with `unsafePerm: false` now fails with an error when pnpm cannot create `node_modules/.tmp`. It used to hang.

- After relaying a signal to a script, pnpm keeps waiting for a process in the script's process group whose main thread has exited while its other threads still run. Linux reports such a process as a zombie, so the wait used to end before those threads finished [pnpm/tasks#56](https://github.com/pnpm/tasks/issues/56).
