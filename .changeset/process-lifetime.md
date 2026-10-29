---
"pacquet": patch
---

Processes that a script or `pnpm exec` command leaves running now outlive pnpm exactly when they would outlive the command run without pnpm. On Windows, a failed command, `Ctrl+C`, or a closed terminal no longer ends programs a script started, such as a browser opened by the `open` package [#17465](https://github.com/pnpm/pnpm/issues/17465). pnpm still ends them when it is killed or crashes.

When a bail stops the scripts still running in a recursive or parallel run, they now receive `SIGTERM` on Unix and are killed only if they are still running two seconds later. On Windows, everything those scripts started is ended, including detached processes whose parent has already exited.
