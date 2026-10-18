---
"pnpm": patch
---

When the reader of pnpm's output exits early, as in `pnpm outdated | head`, pnpm now finishes the command and exits with the command's own exit code [pnpm/tasks#133](https://github.com/pnpm/tasks/issues/133). It used to exit with code 0 at the first write to the closed pipe. That hid the result of commands such as `pnpm outdated` and stopped `pnpm clean` before it removed anything.
